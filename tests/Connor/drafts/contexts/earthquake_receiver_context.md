# EarthquakeReceiverContext

## Purpose

EarthquakeReceiverContext is the boundary adapter for the USGS Earthquake Hazards GeoJSON
feed. It polls the USGS API at a regular interval, translates each returned EarthquakeEvent
into a PositionEvent, and forwards those events into the wider system.

This context fulfills the EventSource role in exactly the same way as every other receiver.
The downstream EventBufferContext must not be able to distinguish which receiver is active.
Running this context alongside any other receiver must be a matter of configuration, not of
changing any downstream code.

The USGS feed provides global earthquake detections updated approximately every minute. Each
response returns all events that occurred within a recent time window. Because the same
event will appear in many successive responses, the receiver deduplicates by event ID before
forwarding to the buffer.

## Role Players

| Role player | Why involved | Expected behaviour |
|---|---|---|
| event_sink | Receives produced PositionEvents | Accepts each successfully mapped and deduplicated earthquake event |

## Role Methods

### event_sink

- **receive_event(event)** Accepts a single PositionEvent and stores it for later querying.

## Props

| Prop | Meaning | Notes |
|---|---|---|
| api_url | URL of the USGS GeoJSON feed to poll | Feed granularity is encoded in the configured URL |
| poll_interval | How frequently the feed is fetched | Once per minute is a sensible default |
| min_magnitude | Optional lower bound on magnitude | Events below this threshold are silently dropped |

## Functionalities

### new

| Started by | Uses | Result |
|---|---|---|
| application startup | event_sink, props | receiver is constructed |

Rules:
- Stores `event_sink`, `api_url`, `poll_interval`, and optional `min_magnitude`.
- The `event_sink` is passed by shared reference because the same EventBufferContext may be shared with other contexts.
- Initializes an empty set of seen event IDs for deduplication.
- Does not start polling during construction.
- Call `start` to begin the polling loop.

| Given | When | Then |
|---|---|---|
| an event sink and valid feed configuration are available | new is called | an EarthquakeReceiverContext is returned with an empty deduplication set |

### start

| Started by | Uses | Result |
|---|---|---|
| application runtime | poll_interval | background polling loop begins |

Rules:
- Begins a continuous background loop.
- Waits for `poll_interval` to elapse before each fetch cycle.
- Calls `fetch_events` on each scheduled iteration.
- Retains the same configuration and deduplication state across iterations.

| Given | When | Then |
|---|---|---|
| a configured receiver | start is called | the receiver begins polling the USGS feed on the configured interval |

### fetch_events

| Started by | Uses | Result |
|---|---|---|
| polling loop | api_url | one feed snapshot is fetched and each feature is handed to `on_event_received` |

Rules:
- Issues a GET request to the configured `api_url`.
- On a successful response, parses the GeoJSON FeatureCollection.
- Calls `on_event_received` for each feature in the collection.
- On failed responses, records a diagnostic error count.
- Waits until the next scheduled interval before retrying after a failure.

| Given | When | Then |
|---|---|---|
| a successful feed response containing five unique events with valid coordinates | fetch_events runs | five features are handed to `on_event_received` and five PositionEvents can be delivered |

### on_event_received

| Started by | Uses | Result |
|---|---|---|
| `fetch_events` | raw_event, event_sink, optional min_magnitude | a mapped PositionEvent is delivered or the event is discarded |

Rules:
- Attempts to parse `raw_event` as an EarthquakeEvent.
- If the event ID has already been seen, discards it silently.
- Records each newly seen event ID for future deduplication.
- If `min_magnitude` is configured and the event's magnitude is below the threshold, discards the event silently.
- Maps latitude and longitude from the event epicenter coordinates.
- Uses the event timestamp as `occurred_at`.
- Sets `source` to `earthquake`.
- Forms `label` from magnitude and place when both are available, and otherwise uses whichever of those values is present.
- Passes the resulting PositionEvent to `event_sink.receive_event`.

| Given | When | Then |
|---|---|---|
| an already-seen earthquake event arrives a second time | on_event_received runs | no PositionEvent is emitted and the duplicate is silently discarded |

## Notes

- The USGS feed uses GeoJSON coordinate order: `[longitude, latitude, depth]`.
- Timestamps are provided as milliseconds since Unix epoch.
- Magnitude is the floating-point `mag` property and may be `null` for very recent events.
- Event ID is carried in the GeoJSON feature `id`.
- The `place` property is a string or `null`.
