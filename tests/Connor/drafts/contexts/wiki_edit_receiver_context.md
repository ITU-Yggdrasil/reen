# WikiEditReceiverContext

## Description

WikiEditReceiverContext is the boundary adapter for the Wikimedia RecentChange event stream.
It maintains a persistent server-sent event (SSE) connection to the Wikimedia stream
endpoint, filters the incoming edit events, resolves geographic coordinates for each edited
article, and forwards a PositionEvent into the wider system for every edit to a geotagged
article.

This context fulfils the EventSource role in exactly the same way as every other receiver.
The downstream EventBufferContext must not be able to distinguish which receiver is active.
Running this context alongside any other receiver must be a matter of configuration, not of
changing any downstream code.

The edit rate across all Wikipedia language editions is high (several edits per second at
peak). Most edits are to articles without geographic coordinates and are discarded early.
The remaining edits require a GeoData API lookup per unique article title to resolve
coordinates. The receiver caches article coordinates to avoid redundant API calls for
articles edited repeatedly within a short window.

---

## Roles

- **event_sink**
  The recipient of produced PositionEvents.
  Fulfilled by EventBufferContext.
  The receiver must not know the concrete type of the sink; it only calls the
  receive_event behaviour on whatever is playing this role.

---

## Props

- **stream_url**
  The URL of the Wikimedia RecentChange SSE endpoint.
  The standard endpoint is https://stream.wikimedia.org/v2/stream/recentchange.

- **geodata_api_url**
  The base URL of the MediaWiki API used to resolve article coordinates via the GeoData
  extension. A per-wiki URL is constructed from this base and the target wiki.

- **reconnect_delay**
  How long to wait before attempting to reconnect after the SSE stream drops.

- **include_bot_edits**
  Whether to emit PositionEvents for edits flagged as bot edits. Defaults to false.
  Bot edits are common and can produce a disproportionate number of events in certain
  regions; excluding them focuses the feed on human activity.

---

## Role methods

### event_sink

- **receive_event(event)**
  Accepts a single PositionEvent and stores it for later querying.
  The receiver calls this once per edit that resolves to a geographic coordinate.

---

## Functionalities

- **new(event_sink, stream_url, geodata_api_url, reconnect_delay, include_bot_edits)**
  Constructs a WikiEditReceiverContext with the given role player and props.
  The event_sink is passed as a shared reference — the same EventBufferContext instance
  is also held by AggregationContext (and by any other active receiver). The application
  must not give up sole ownership of the buffer when passing it here.
  Stores all provided values and initialises an empty coordinate cache.
  Does not open the stream connection. Call start to begin receiving events.

- **start**
  Opens the SSE connection to the configured stream_url and begins reading events.
  Runs continuously in the background.
  Reconnects automatically after any disconnection.

- **on_event_received(raw_event)**
  Called internally each time an SSE message arrives on the stream.
  Parses the raw message as a WikiEdit.
  If the wiki is not a Wikipedia project (i.e., not in the pattern "*wiki" excluding
  "wikimedia"), discards the event silently.
  If the namespace is not 0 (the article namespace), discards the event silently.
  If include_bot_edits is false and the bot flag is set, discards the event silently.
  Otherwise, calls resolve_coordinates with the wiki and title.
  If coordinates are returned, maps the edit to a PositionEvent:
  - latitude and longitude are the resolved article coordinates,
  - occurred_at is taken from the edit's timestamp,
  - source is set to "wiki",
  - label is set to the article title.
  Passes the resulting PositionEvent to event_sink.receive_event.

- **resolve_coordinates(wiki, title)**
  Checks the coordinate cache for an entry matching the wiki and title.
  If a cached entry exists and has not expired, returns it without making an API call.
  Otherwise, issues a GET request to the GeoData API for the given wiki and title.
  If the API response includes coordinates, stores them in the cache and returns them.
  If the API response indicates no coordinates exist for this article, stores a negative
  result in the cache (to avoid re-querying for the same missing title) and returns absent.
  If the API call fails, returns absent without caching the result (allows retry next time
  the same article is edited).

- **on_stream_disconnected**
  Called internally when the SSE connection drops or the server closes it.
  Waits for the configured reconnect_delay and then calls start to reconnect.
  Does not discard the event_sink, coordinate cache, or any configuration.

---

## Parsing details

- The Wikimedia SSE stream wraps each event as a JSON object in the `data` field of the
  SSE message.
- The `type` field distinguishes edits ("edit"), new pages ("new"), and other change types.
  Only "edit" and "new" types produce PositionEvents.
- Timestamps in the stream are Unix epoch integers (seconds).
- The GeoData API is queried as:
  `{wiki_api_base}/w/api.php?action=query&titles={title}&prop=coordinates&format=json`
  where `{wiki_api_base}` is derived from the wiki identifier (e.g., "enwiki" maps to
  `https://en.wikipedia.org`).
- The coordinate cache entry for a title should expire after a period long enough to
  avoid churning the API but short enough that newly geotagged articles eventually appear.
  A cache TTL of several hours is a sensible default.

---

## Acceptance examples

- Given an SSE event arrives for an edit to the article "Eiffel Tower" on enwiki, when
  on_event_received runs and the GeoData API returns coordinates for that article, then a
  PositionEvent labelled "Eiffel Tower" at the Paris coordinates is delivered to event_sink.
- Given an SSE event arrives for an edit to a non-article page (namespace != 0), when
  on_event_received runs, then no event is delivered and the record is silently discarded.
- Given an SSE event arrives for a bot edit and include_bot_edits is false, when
  on_event_received runs, then no event is delivered and the record is silently discarded.
- Given an SSE event arrives for an edit to an article with no geographic coordinates in
  GeoData, when resolve_coordinates runs, then no event is delivered and a negative cache
  entry is stored to prevent re-querying the same title.
- Given the same article is edited twice in quick succession, when on_event_received runs
  for the second edit, then the cached coordinates are used and no GeoData API call is made.
- Given the SSE stream disconnects, when on_stream_disconnected runs, then a reconnection
  attempt is made after reconnect_delay has elapsed.
