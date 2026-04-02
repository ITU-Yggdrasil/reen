# FlightReceiverContext

## Purpose

FlightReceiverContext is the boundary adapter for the OpenSky Network REST API. It polls
the API at a regular interval, translates each returned FlightPosition into a
PositionEvent, and forwards those events into the wider system.

This context fulfills the EventSource role in exactly the same way as other receivers.
The downstream EventBufferContext must not be able to distinguish which receiver is active.
Running this context alongside any other receiver must be a matter of configuration, not of
changing any downstream code.

## Role Players

| Role player | Why involved | Expected behaviour |
|---|---|---|
| event_sink | Receives produced PositionEvents | Accepts each successfully mapped flight event |

## Role Methods

### event_sink

- **receive_event(event)** Accepts a single PositionEvent and stores it for later querying.

## Props

| Prop | Meaning | Notes |
|---|---|---|
| api_url | Base URL of the OpenSky Network states endpoint | Queried on each poll cycle |
| api_token | Optional bearer token for authenticated OpenSky access | When absent, requests are sent without an Authorization header |
| poll_interval | How frequently the API is queried | Default should respect anonymous rate limits |
| bounding_box | Optional geographic restriction for the query | Inline `min_latitude`, `max_latitude`, `min_longitude`, `max_longitude` values |

## Functionalities

### new

| Started by | Uses | Result |
|---|---|---|
| application startup | event_sink, props | receiver is constructed |

Rules:
- Stores `event_sink`, `api_url`, optional `api_token`, `poll_interval`, and optional `bounding_box`.
- The `event_sink` is passed by shared reference because the same EventBufferContext may be shared with other contexts.
- Does not start polling during construction.
- Call `start` to begin the polling loop.

| Given | When | Then |
|---|---|---|
| an event sink and valid OpenSky configuration are available | new is called | a FlightReceiverContext is returned without polling yet |

### start

| Started by | Uses | Result |
|---|---|---|
| application runtime | poll_interval | background polling loop begins |

Rules:
- Begins a continuous background loop.
- Waits for `poll_interval` to elapse before each fetch cycle.
- Calls `fetch_positions` on each scheduled iteration.
- Retains the same configuration and event sink across iterations.

| Given | When | Then |
|---|---|---|
| a configured receiver | start is called | the receiver begins polling the flight feed on the configured interval |

### fetch_positions

| Started by | Uses | Result |
|---|---|---|
| polling loop | api_url, optional api_token, optional bounding_box | one API snapshot is fetched and each record is handed to `on_position_received` |

Rules:
- Issues a GET request to the configured `api_url`.
- Applies `bounding_box` to the request when configured.
- Includes `Authorization: Bearer <token>` when `api_token` is present.
- On a successful response, iterates over the returned flight state records.
- Calls `on_position_received` for each returned record.
- On failed or rate-limited responses, records a diagnostic error count.
- Waits until the next scheduled interval before retrying after a failure.

| Given | When | Then |
|---|---|---|
| a successful API response containing three valid positions | fetch_positions runs | three records are handed to `on_position_received` and three PositionEvents can be delivered |

### on_position_received

| Started by | Uses | Result |
|---|---|---|
| `fetch_positions` | raw_position, event_sink | a mapped PositionEvent is delivered or the record is discarded |

Rules:
- Attempts to parse `raw_position` as a FlightPosition.
- If latitude or longitude is absent, discards the record silently.
- Maps latitude and longitude directly from the flight position when present.
- Uses the flight position timestamp as `occurred_at`.
- Sets `source` to `flight`.
- Uses the callsign as `label` when it is present.
- Falls back to the literal label `flight` when the callsign is absent.
- Passes the resulting PositionEvent to `event_sink.receive_event`.

| Given | When | Then |
|---|---|---|
| a position record with no latitude | on_position_received runs | no PositionEvent is emitted and the record is silently discarded |

## Notes

- OpenSky state responses are snapshots rather than event streams.
- The receiver emits one PositionEvent per successfully mapped aircraft state in each poll cycle.
