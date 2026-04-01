# Application

## Description

The application is the entry point that wires the system together and starts it running.
It reads configuration, constructs every context, connects them through their roles, and
hands off control. Once started, it has no further active role - the contexts it has
assembled run autonomously.

The application exposes two HTTP endpoints, both served by MetricsContext on a single
HTTP listener:

**`/metrics` - Prometheus scrape endpoint**
When Prometheus scrapes this path, MetricsContext calls `aggregator.produce_rates`, which
returns a collection of EventRate values. Each EventRate carries a GeographicCell (with its
four boundary values) and an events_per_minute figure. MetricsContext encodes each of those
into a Prometheus text-format gauge line and writes the full result back as the response body.
The wire format is the Prometheus text exposition format; the content-type header must
identify it as such.

**`/query` - JSON query endpoint**
When a client calls this path, MetricsContext queries EventBufferContext for the current
live events, optionally filtered by source. It serialises the result as a JSON array of
dashboard events. Each element contains:
- latitude
- longitude
- occurred_at
- source
- label
The `source` field is taken from `PositionEvent.source`, which is set by the receiver
that created the event. Source filtering also operates on `PositionEvent.source`.
If the `source` query parameter is absent, `flight`, `lightning`, `earthquake`, `aisstream`,
`adsb`, or `wiki`, the corresponding filter is applied. Any other `source` value is
treated as absent and returns events from all sources.
The wire format is `application/json`.

For `/metrics`, the data type flowing from AggregationContext into MetricsContext is a
collection of EventRate values. For `/query`, the data flowing into MetricsContext comes
from EventBufferContext as live PositionEvent records. MetricsContext is solely responsible
for serialisation and performs no domain computation on either result.

The application can start up to six live external feeds: lightning, flight, earthquake,
AISStream, ADS-B, and wiki edits. Each one is enabled or disabled independently through
configuration.

---

## Startup sequence

1. Load configuration from `.env` and the process environment, with process environment
   variables taking precedence. Read grid resolution, time window, server bind address,
   endpoint paths, and source-specific connection settings.

2. Construct GridContext from the configured GridResolution.

3. Construct EventBufferContext with the configured TimeWindow and wrap it in shared
   ownership. All subsequent steps that need the buffer receive a shared reference to
   the same instance - they do not receive sole ownership.

4. Construct AggregationContext, passing a shared reference to EventBufferContext into
   the buffer role and GridContext into the grid role.

5. Construct MetricsContext, casting AggregationContext into the aggregator role.
   Bind and start the HTTP listener.

6. If a lightning websocket URL is configured:
   - Construct LightningReceiverContext, passing a shared reference to EventBufferContext
     into the event_sink role.
   - Each accepted strike is mapped into a PositionEvent with `source = lightning`.
   - Start the receiver.

7. If a flight API URL is configured:
   - Construct FlightReceiverContext, passing a shared reference to EventBufferContext
     into the event_sink role and passing OPENSKY_API_TOKEN into the api_token prop.
   - If OPENSKY_API_TOKEN is present, FlightReceiverContext sends it on each OpenSky
     request as `Authorization: Bearer <token>`. If absent, no Authorization header is sent.
   - Each accepted aircraft position is mapped into a PositionEvent with `source = flight`.
   - Start the receiver.

8. If an earthquake API URL is configured:
   - Construct EarthquakeReceiverContext, passing a shared reference to EventBufferContext
     into the event_sink role.
   - Apply the configured poll interval and optional minimum magnitude threshold.
   - Each accepted earthquake is mapped into a PositionEvent with `source = earthquake`.
   - Start the receiver.

9. If an AISStream websocket URL is configured and an AISStream API key is configured:
   - Construct AISStreamContext using the configured websocket URL, API key, and reconnect delay.
   - Construct AISStreamReceiverContext, passing AISStreamContext into the `aisstream` role
     and a shared reference to EventBufferContext into the `event_sink` role.
   - Apply the configured optional bounding box.
   - AISStreamReceiverContext requests only `PositionReport` and
     `StandardClassBPositionReport` from AISStream.
   - Each accepted AISStream position is mapped into a PositionEvent with `source = aisstream`.
   - Start the receiver.

10. If an ADS-B API URL is configured and an ADS-B API key is configured:
   - Construct AdsbReceiverContext, passing a shared reference to EventBufferContext
     into the event_sink role.
   - Apply the configured poll interval and optional bounding box.
   - Each accepted aircraft state is mapped into a PositionEvent with `source = adsb`.
   - Start the receiver.

11. If a wiki stream URL is configured:
   - Construct WikiEditReceiverContext, passing a shared reference to EventBufferContext
     into the event_sink role.
   - Apply the configured GeoData API template, reconnect delay, and bot-edit inclusion rule.
   - Each accepted geotagged Wikipedia edit is mapped into a PositionEvent with `source = wiki`.
   - Start the receiver.

12. If simulation is enabled:
   - Start the synthetic event loop that emits lightning, flight, earthquake, AISStream,
     ADS-B, and wiki PositionEvents into EventBufferContext at the configured interval,
     setting each event source to match the simulated feed.

13. Block until the process receives a shutdown signal.

---

## Configuration surface

- **GRID_DEGREES_LAT** - degrees of latitude per cell; default `10`.
- **GRID_DEGREES_LON** - degrees of longitude per cell; default `10`.
- **TIME_WINDOW_MINUTES** - how long events are retained; default `5`.
- **BIND_ADDRESS** - host and port for the HTTP server. Required.
- **METRICS_PATH** - URL path for the Prometheus endpoint; default `/metrics`.
- **QUERY_PATH** - URL path for the JSON endpoint; default `/query`.
- **LIGHTNING_WEBSOCKET_URL** - Blitzortung websocket endpoint. Leave blank to disable the lightning receiver.
- **LIGHTNING_RECONNECT_DELAY_SECS** - wait time before reconnecting after a dropped websocket; default `5`.
- **FLIGHT_API_URL** - OpenSky states endpoint. Leave blank to disable the flight receiver.
- **OPENSKY_API_TOKEN** - optional bearer token for authenticated OpenSky access.
- **FLIGHT_POLL_INTERVAL_SECS** - how frequently to query OpenSky; default `60`.
- **FLIGHT_BOUNDING_BOX** - optional lat/lon bounds as `min_lat,max_lat,min_lon,max_lon`.
- **EARTHQUAKE_API_URL** - USGS GeoJSON feed URL. Leave blank to disable the earthquake receiver.
- **EARTHQUAKE_POLL_INTERVAL_SECS** - how frequently to query the earthquake feed; default `60`.
- **EARTHQUAKE_MIN_MAGNITUDE** - optional minimum earthquake magnitude filter.
- **AISSTREAM_WEBSOCKET_URL** - AISStream websocket endpoint. Leave blank to disable the AISStream receiver.
- **AISSTREAM_API_KEY** - API key used when subscribing to AISStream.
- **AISSTREAM_RECONNECT_DELAY_SECS** - wait time before reconnecting after a dropped AISStream connection; default `5`.
- **AISSTREAM_BOUNDING_BOX** - optional lat/lon bounds as `min_lat,max_lat,min_lon,max_lon`.
- **ADSB_API_URL** - ADS-B aircraft endpoint. Leave blank to disable the ADS-B receiver.
- **ADSB_API_KEY** - API key sent with ADS-B requests.
- **ADSB_POLL_INTERVAL_SECS** - how frequently to query the ADS-B feed; default `10`.
- **ADSB_BOUNDING_BOX** - optional lat/lon bounds as `min_lat,max_lat,min_lon,max_lon`.
- **WIKI_STREAM_URL** - Wikimedia RecentChange stream URL. Leave blank to disable the wiki receiver.
- **WIKI_GEODATA_API_URL** - GeoData API URL template; supports `{host}` replacement; default `https://{host}/w/api.php`.
- **WIKI_RECONNECT_DELAY_SECS** - wait time before reconnecting after a dropped wiki stream; default `5`.
- **WIKI_INCLUDE_BOT_EDITS** - whether bot edits should produce events; default `false`.
- **SIMULATE_EVENTS** - whether to emit synthetic lightning, flight, earthquake, AISStream,
  ADS-B, and wiki events locally; default `false`.
- **SIMULATE_INTERVAL_SECS** - how frequently to emit synthetic events; default `2`.
