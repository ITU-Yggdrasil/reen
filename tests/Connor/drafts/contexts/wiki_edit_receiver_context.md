# WikiEditReceiverContext

## Purpose

WikiEditReceiverContext is the boundary adapter for the Wikimedia RecentChange event
stream. It maintains a persistent server-sent event connection to the Wikimedia endpoint,
filters the incoming edit events, resolves geographic coordinates for each edited article,
and forwards a PositionEvent into the wider system for every edit to a geotagged article.

This context fulfills the EventSource role in exactly the same way as every other receiver.
The downstream EventBufferContext must not be able to distinguish which receiver is active.
Running this context alongside any other receiver must be a matter of configuration, not of
changing any downstream code.

The edit rate across Wikipedia is high. Most edits are to articles without geographic
coordinates and are discarded early. The remaining edits require a GeoData lookup per
unique article title, so the receiver caches coordinate lookups to avoid redundant API
calls for articles edited repeatedly within a short window.

## Role Players

| Role player | Why involved | Expected behaviour |
|---|---|---|
| event_sink | Receives produced PositionEvents | Accepts each edit that resolves to a geographic coordinate |

## Role Methods

### event_sink

- **receive_event(event)** Accepts a single PositionEvent and stores it for later querying.

## Props

| Prop | Meaning | Notes |
|---|---|---|
| stream_url | URL of the Wikimedia RecentChange SSE endpoint | Standard endpoint is `https://stream.wikimedia.org/v2/stream/recentchange` |
| geodata_api_url | Base URL of the MediaWiki API used to resolve article coordinates | A per-wiki URL is derived from this base and the target wiki |
| reconnect_delay | How long to wait before reconnecting after the stream drops | Applied after disconnects |
| include_bot_edits | Whether bot edits should emit PositionEvents | Defaults to false |

## Functionalities

### new

| Started by | Uses | Result |
|---|---|---|
| application startup | event_sink, props | receiver is constructed |

Rules:
- Stores `event_sink`, `stream_url`, `geodata_api_url`, `reconnect_delay`, and `include_bot_edits`.
- The `event_sink` is passed by shared reference because the same EventBufferContext may be shared with other contexts.
- Initializes an empty coordinate cache.
- Does not open the stream connection during construction.
- Call `start` to begin receiving events.

| Given | When | Then |
|---|---|---|
| an event sink and valid Wikimedia configuration are available | new is called | a WikiEditReceiverContext is returned with an empty coordinate cache |

### start

| Started by | Uses | Result |
|---|---|---|
| application runtime | stream_url | SSE receive loop begins |

Rules:
- Opens the SSE connection to the configured `stream_url`.
- Begins reading events continuously in the background.
- Reconnects automatically after any disconnection by delegating to `on_stream_disconnected`.
- Retains the same coordinate cache and configuration across reconnects.

| Given | When | Then |
|---|---|---|
| a reachable RecentChange endpoint | start is called | the receiver begins consuming Wikimedia SSE messages |

### on_event_received

| Started by | Uses | Result |
|---|---|---|
| SSE receive loop | raw_event, include_bot_edits, resolve_coordinates, event_sink | a mapped PositionEvent is delivered or the event is discarded |

Rules:
- Parses the raw SSE message as a WikiEdit.
- Discards the event silently if the wiki is not a Wikipedia project matching `*wiki` other than `wikimedia`.
- Discards the event silently if the namespace is not `0`.
- Discards the event silently if `include_bot_edits` is false and the bot flag is set.
- Calls `resolve_coordinates` with the wiki and article title for events that survive filtering.
- If coordinates are returned, maps them to PositionEvent latitude and longitude.
- Uses the edit timestamp as `occurred_at`.
- Sets `source` to `wiki`.
- Sets `label` to the article title.
- Passes the resulting PositionEvent to `event_sink.receive_event`.

| Given | When | Then |
|---|---|---|
| an edit to `Eiffel Tower` on `enwiki` and a successful coordinate lookup | on_event_received runs | a PositionEvent labeled `Eiffel Tower` is delivered at the resolved coordinates |

### resolve_coordinates

| Started by | Uses | Result |
|---|---|---|
| `on_event_received` | coordinate cache, geodata_api_url, wiki, title | coordinates are returned from cache or API, or absence is returned |

Rules:
- Checks the coordinate cache for an entry matching `wiki` and `title`.
- If a cached entry exists and has not expired, returns it without making an API call.
- Otherwise, issues a GET request to the GeoData API for the given wiki and title.
- If the API response includes coordinates, stores them in the cache and returns them.
- If the API response indicates that the article has no coordinates, stores a negative cache entry and returns absent.
- If the API call fails, returns absent without caching the failure so a later edit can retry.

| Given | When | Then |
|---|---|---|
| the same article is edited twice in quick succession | resolve_coordinates runs for the second edit | the cached coordinates are returned and no GeoData API call is made |

### on_stream_disconnected

| Started by | Uses | Result |
|---|---|---|
| SSE receive loop | reconnect_delay | reconnect attempt is deferred |

Rules:
- Runs when the SSE connection drops or the server closes it.
- Waits for the configured `reconnect_delay`.
- Calls `start` to reconnect after the delay.
- Retains the existing `event_sink`, coordinate cache, and configuration.

| Given | When | Then |
|---|---|---|
| the SSE stream disconnects | on_stream_disconnected runs | a reconnection attempt is made after `reconnect_delay` has elapsed |

## Notes

- The Wikimedia stream wraps each event as a JSON object in the SSE `data` field.
- The `type` field distinguishes edits, new pages, and other change types; only `edit` and `new` are eligible to produce PositionEvents.
- Timestamps in the stream are Unix epoch integers in seconds.
- The GeoData API request shape is `{wiki_api_base}/w/api.php?action=query&titles={title}&prop=coordinates&format=json`, where `{wiki_api_base}` is derived from the wiki identifier, for example `enwiki` to `https://en.wikipedia.org`.
- Coordinate cache entries should expire after long enough to reduce API churn but short enough for newly geotagged articles to appear; several hours is a sensible default.
