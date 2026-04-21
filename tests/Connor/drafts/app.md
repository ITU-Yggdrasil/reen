# Application

## Application Kind

`service_app`

## Purpose

The application is the entry point that wires the system together and starts it running. It reads configuration, constructs every context and data type, connects them through their roles, and hands off control. Once started, it has no further active role — the contexts it has assembled run autonomously.

## Runtime Topology

The application runs as one long-lived backend process. It starts one HTTP listener for the request/response surface and may also start up to six live feed workers for lightning, flight, earthquake, AISStream, ADS-B, and wiki edits.

## Command Interface

No CLI subcommands are specified. Startup behavior is controlled through configuration and environment variables.

## Transport Surface

The application exposes two HTTP endpoints, both served by Metrics on a single HTTP listener:

**`/metrics` — Prometheus scrape endpoint**
When Prometheus scrapes this path, Metrics asks the aggregator for current rates, which returns a collection of EventRate values. Each EventRate carries a GeographicCell and an events_per_minute figure. Metrics encodes each into a Prometheus text-format gauge line and writes the full result as the response body.

**`/query` — JSON query endpoint**
When a client calls this path, Metrics queries EventBuffer for the current live events, optionally filtered by source. It serialises the result as a JSON array of dashboard event objects. Each element contains latitude, longitude, occurred_at, source, and label. The source field is the canonical event source identifier set by the receiver that created the event. Source filtering also operates on this field. If the source query parameter is absent or not one of "flight", "lightning", "earthquake", "aisstream", "adsb", or "wiki", events from all sources are returned.

## Static Surface

Not applicable. This draft specifies a backend service surface only.

## Collaborators and Wiring

| Collaborator | Responsibility |
|---|---|
| Grid | Maps geographic positions into configured grid cells. |
| EventBuffer | Stores the shared live-event buffer used by downstream readers and receivers. |
| Aggregation | Produces rate snapshots from buffered events and grid assignment. |
| Metrics | Serves the HTTP endpoints and serialises the response bodies. |
| LightningReceiver | Connects the lightning feed and forwards accepted events into the shared buffer. |
| FlightReceiver | Polls the flight feed and forwards accepted events into the shared buffer. |
| EarthquakeReceiver | Polls the earthquake feed and forwards accepted events into the shared buffer. |
| AISStreamReceiver | Consumes AISStream messages and forwards accepted events into the shared buffer. |
| AdsbReceiver | Polls the ADS-B feed and forwards accepted events into the shared buffer. |
| WikiEditReceiver | Consumes the wiki edit stream and forwards accepted events into the shared buffer. |

---

## Startup Sequence

1. Load configuration from the environment. Read grid resolution, time window, server bind address, endpoint paths, and source-specific connection settings.

2. Construct Grid from the configured GridResolution.

3. Construct EventBuffer with the configured TimeWindow. All subsequent steps that need the buffer receive a shared reference to the same instance.

4. Construct Aggregation, passing EventBuffer into the buffer role and Grid into the grid role.

5. Construct Metrics, passing Aggregation into the aggregator role and EventBuffer into the buffer role. Bind and start the HTTP listener.

6. If a lightning websocket URL is configured, construct LightningReceiver passing EventBuffer into the event_sink role, and start it. Each accepted strike becomes a PositionEvent with source "lightning".

7. If a flight API URL is configured, construct FlightReceiver passing EventBuffer into the event_sink role and the optional API token into the api_token prop. Each accepted aircraft position becomes a PositionEvent with source "flight". Start the receiver.

8. If an earthquake API URL is configured, construct EarthquakeReceiver passing EventBuffer into the event_sink role. Apply the configured poll interval and optional minimum magnitude threshold. Each accepted earthquake becomes a PositionEvent with source "earthquake". Start the receiver.

9. If an AISStream websocket URL and API key are both configured, construct the AISStream transport and then AISStreamReceiver, passing the transport into the aisstream role and EventBuffer into the event_sink role. Apply the optional bounding box. The receiver requests only PositionReport and StandardClassBPositionReport message families. Each accepted position becomes a PositionEvent with source "aisstream". Start the receiver.

10. If an ADS-B API URL and API key are both configured, construct AdsbReceiver passing EventBuffer into the event_sink role. Apply the configured poll interval and optional bounding box. Each accepted aircraft state becomes a PositionEvent with source "adsb". Start the receiver.

11. If a wiki stream URL is configured, construct WikiEditReceiver passing EventBuffer into the event_sink role. Apply the configured GeoData API template, reconnect delay, and bot-edit inclusion rule. Each accepted geotagged Wikipedia edit becomes a PositionEvent with source "wiki". Start the receiver.

12. If simulation is enabled, start the synthetic event loop that emits PositionEvents for all six sources into EventBuffer at the configured interval.

13. Block until the process receives a shutdown signal.

---

## Configuration Surface

- GRID_DEGREES_LAT — degrees of latitude per cell; default 10
- GRID_DEGREES_LON — degrees of longitude per cell; default 10
- TIME_WINDOW_MINUTES — how long events are retained; default 5
- BIND_ADDRESS — host and port for the HTTP server; required
- METRICS_PATH — URL path for the Prometheus endpoint; default /metrics
- QUERY_PATH — URL path for the JSON endpoint; default /query
- LIGHTNING_WEBSOCKET_URL — Blitzortung websocket endpoint; leave blank to disable
- LIGHTNING_RECONNECT_DELAY_SECS — reconnect wait time; default 5
- FLIGHT_API_URL — OpenSky states endpoint; leave blank to disable
- OPENSKY_API_TOKEN — optional bearer token for authenticated OpenSky access
- FLIGHT_POLL_INTERVAL_SECS — poll frequency for OpenSky; default 60
- FLIGHT_BOUNDING_BOX — optional geographic restriction
- EARTHQUAKE_API_URL — USGS GeoJSON feed URL; leave blank to disable
- EARTHQUAKE_POLL_INTERVAL_SECS — poll frequency for earthquakes; default 60
- EARTHQUAKE_MIN_MAGNITUDE — optional minimum magnitude filter
- AISSTREAM_WEBSOCKET_URL — AISStream websocket endpoint; leave blank to disable
- AISSTREAM_API_KEY — API key for AISStream subscription
- AISSTREAM_RECONNECT_DELAY_SECS — reconnect wait time; default 5
- AISSTREAM_BOUNDING_BOX — optional geographic restriction
- ADSB_API_URL — ADS-B aircraft endpoint; leave blank to disable
- ADSB_API_KEY — API key for ADS-B requests
- ADSB_POLL_INTERVAL_SECS — poll frequency for ADS-B; default 10
- ADSB_BOUNDING_BOX — optional geographic restriction
- WIKI_STREAM_URL — Wikimedia RecentChange stream URL; leave blank to disable
- WIKI_GEODATA_API_URL — GeoData API URL template; default uses per-wiki host substitution
- WIKI_RECONNECT_DELAY_SECS — reconnect wait time; default 5
- WIKI_INCLUDE_BOT_EDITS — whether bot edits produce events; default false
- SIMULATE_EVENTS — whether to emit synthetic events locally; default false
- SIMULATE_INTERVAL_SECS — synthetic event frequency; default 2
