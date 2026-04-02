# AdsbReceiverContext

## Purpose

AdsbReceiverContext is the boundary adapter for the ADS-B Exchange REST API. It polls the
API at a regular interval, translates each returned AircraftState into a PositionEvent, and
forwards those events into the wider system.

This context fulfills the EventSource role in exactly the same way as every other receiver.
The downstream EventBufferContext must not be able to distinguish which receiver is active.
Running this context alongside any other receiver must be a matter of configuration, not of
changing any downstream code.

ADS-B Exchange provides near-real-time positions of aircraft worldwide, aggregated from a
volunteer network of ground-based receivers. Each response is a snapshot of all currently
tracked aircraft, not a stream of individual updates. The receiver polls on a fixed interval
and emits one PositionEvent per aircraft per poll cycle, provided the aircraft has a valid
position fix.

## Role Players

| Role player | Why involved | Expected behaviour |
|---|---|---|
| event_sink | Receives produced PositionEvents | Accepts each successfully mapped aircraft event |

## Role Methods

### event_sink

- **receive_event(event)** Accepts a single PositionEvent and stores it for later querying.

## Props

| Prop | Meaning | Notes |
|---|---|---|
| api_url | Base URL of the ADS-B Exchange aircraft endpoint | When `bounding_box` is absent, the global snapshot is requested |
| api_key | API key used to authenticate with ADS-B Exchange | Sent as a request header |
| poll_interval | How frequently the API is queried | Default should respect the configured rate-limit tier |
| bounding_box | Optional geographic restriction for the query | Inline `min_latitude`, `max_latitude`, `min_longitude`, `max_longitude` values |

## Functionalities

### new

| Started by | Uses | Result |
|---|---|---|
| application startup | event_sink, props | receiver is constructed |

Rules:
- Stores `event_sink`, `api_url`, `api_key`, `poll_interval`, and optional `bounding_box`.
- The `event_sink` is passed by shared reference because the same EventBufferContext may be shared with other contexts.
- Does not start polling during construction.
- Call `start` to begin the polling loop.

| Given | When | Then |
|---|---|---|
| an event sink and valid configuration are available | new is called | an AdsbReceiverContext is returned without polling yet |

### start

| Started by | Uses | Result |
|---|---|---|
| application runtime | poll_interval | background polling loop begins |

Rules:
- Begins a continuous background loop.
- Waits for `poll_interval` to elapse before each fetch cycle.
- Calls `fetch_aircraft` on each scheduled iteration.
- Retains the same configuration and event sink across iterations.

| Given | When | Then |
|---|---|---|
| a configured receiver | start is called | the receiver begins polling on the configured interval |

### fetch_aircraft

| Started by | Uses | Result |
|---|---|---|
| polling loop | api_url, api_key, optional bounding_box | one API snapshot is fetched and each record is handed to `on_aircraft_received` |

Rules:
- Issues a GET request to the configured `api_url`.
- Includes `api_key` as a request header.
- Applies `bounding_box` to the request when configured.
- On a successful response, iterates the returned aircraft records from the `ac` array.
- Calls `on_aircraft_received` for each returned record.
- On failed or rate-limited responses, records a diagnostic error count.
- Waits until the next scheduled interval before retrying after a failure.

| Given | When | Then |
|---|---|---|
| a successful response containing four aircraft with valid coordinates | fetch_aircraft runs | four records are handed to `on_aircraft_received` and four PositionEvents can be delivered |

### on_aircraft_received

| Started by | Uses | Result |
|---|---|---|
| `fetch_aircraft` | raw_state, event_sink | a mapped PositionEvent is delivered or the record is discarded |

Rules:
- Attempts to parse `raw_state` as an AircraftState.
- If latitude or longitude is absent, discards the record silently.
- Maps latitude and longitude directly from the aircraft state when present.
- Uses the state's timestamp as `occurred_at`.
- Sets `source` to `adsb`.
- Uses the callsign as `label` when it is present and non-blank.
- Falls back to the ICAO hex code as `label` when the callsign is absent or blank.
- Passes the resulting PositionEvent to `event_sink.receive_event`.

| Given | When | Then |
|---|---|---|
| an aircraft record with blank callsign and valid coordinates | on_aircraft_received runs | a PositionEvent is emitted with the ICAO hex code as its label |

## Notes

- The ADS-B Exchange API returns aircraft records in an `ac` array inside a JSON object.
- Altitude may be numeric or the string `"ground"`.
- Latitude and longitude are floating-point numbers; absence means no field or `null`.
- Timestamp is usually a Unix epoch integer in seconds, and the envelope `now` value can be used when a per-aircraft timestamp is absent.
- Callsign may be an empty string rather than absent; both cases are treated as missing.
