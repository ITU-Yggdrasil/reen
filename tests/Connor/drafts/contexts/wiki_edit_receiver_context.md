# Context: WikiEditReceiverContext

## Purpose

WikiEditReceiver is the boundary adapter for the Wikimedia RecentChange event stream. It maintains a persistent connection to the Wikimedia endpoint, filters incoming edit events, resolves geographic coordinates for each edited article, and forwards a PositionEvent for every edit to a geotagged article. The edit rate across Wikipedia is high; most edits are to articles without geographic coordinates and are discarded early. The receiver caches coordinate lookups to avoid redundant API calls for articles edited repeatedly within a short window.

## Role Players

- event_sink: EventBufferContext — receives each edit that resolves to a geographic coordinate as a PositionEvent

## Props

- stream_url: URL of the Wikimedia RecentChange SSE endpoint
- geodata_api_url: base URL of the MediaWiki API used to resolve article coordinates; a per-wiki URL is derived from this base and the target wiki
- reconnect_delay: how long to wait before reconnecting after the stream drops
- include_bot_edits: whether bot edits should emit PositionEvents; defaults to false

## Role Methods

### event_sink
- receive_event: accepts a single PositionEvent and stores it for later querying

## Functionalities

### new
Stores the event sink, stream URL, GeoData API URL, reconnect delay, and bot-edit inclusion rule. Initializes an empty coordinate cache. Does not open the stream connection during construction.

### start
Opens the SSE connection to the configured stream URL. Begins reading events continuously in the background. Reconnects automatically after any disconnection. Retains the same coordinate cache and configuration across reconnects.

### on_event_received
Parses the raw SSE message as a WikiEdit. Discards the event silently if the wiki is not a Wikipedia project (matching wikis ending in "wiki" other than "wikimedia"). Discards the event silently if the namespace is not 0 (the article namespace). Discards the event silently if bot-edit inclusion is disabled and the bot flag is set. For events that survive filtering, resolves geographic coordinates for the wiki and article title. If coordinates are returned, maps them to the PositionEvent latitude and longitude. Uses the edit timestamp as occurred_at. Sets source to "wiki". Sets label to the article title. Delivers the resulting PositionEvent to the event sink.

### resolve_coordinates
Checks the coordinate cache for an entry matching the wiki and title. If a cached entry exists and has not expired, returns it without making an API call. Otherwise, issues a GET request to the GeoData API for the given wiki and title. If the API response includes coordinates, stores them in the cache and returns them. If the response indicates the article has no coordinates, stores a negative cache entry and returns absent. If the API call fails, returns absent without caching the failure so a later edit can retry. Cache entries expire after several hours.

### on_stream_disconnected
Runs when the SSE connection drops or the server closes it. Waits for the configured reconnect delay. Reconnects by calling start again. Retains the existing event sink, coordinate cache, and configuration.

## Scenarios

### geotagged article produces event
Given: an edit to "Eiffel Tower" on "enwiki" and a successful coordinate lookup
When: on_event_received runs
Then: a PositionEvent labelled "Eiffel Tower" is delivered at the resolved coordinates

### cached coordinates avoid API call
Given: the same article is edited twice in quick succession
When: resolve_coordinates runs for the second edit
Then: the cached coordinates are returned and no GeoData API call is made

### non-Wikipedia wiki is discarded
Given: an edit on "wikimedia" (not a Wikipedia project)
When: on_event_received runs
Then: the event is discarded

### bot edit is discarded when excluded
Given: an edit flagged as a bot edit and include_bot_edits is false
When: on_event_received runs
Then: the event is discarded
