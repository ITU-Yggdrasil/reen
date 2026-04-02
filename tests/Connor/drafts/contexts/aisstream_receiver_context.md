# AISStreamReceiverContext

## Purpose

AISStreamReceiverContext turns raw AISStream position messages into feed-agnostic
PositionEvents for the rest of the system.

It does not own the websocket connection, authentication, or reconnect behavior itself.
Those concerns belong to the collaborator playing the `aisstream` role. This context asks
that collaborator for an AISStream subscription limited to the included position-message
types, receives the resulting boundary records, and forwards mapped PositionEvents into the
wider system.

This context fulfills the EventSource role in exactly the same way as every other receiver.
The downstream EventBufferContext must not be able to distinguish which receiver is active.
Running this context alongside any other receiver must be a matter of configuration, not of
changing any downstream code.

## Role Players

| Role player | Why involved | Expected behaviour |
|---|---|---|
| aisstream | Owns AISStream communication and subscription lifecycle | Starts the upstream subscription and delivers only subscribed messages |
| event_sink | Receives produced PositionEvents | Accepts each successfully mapped vessel event |

## Role Methods

### aisstream

- **start_position_subscription(message_sink, bounding_box, filter_message_types)** Starts or refreshes an AISStream subscription and delivers matching raw messages to the supplied sink. For this receiver the positive filter must be exactly `PositionReport` and `StandardClassBPositionReport`.

### event_sink

- **receive_event(event)** Accepts a single PositionEvent and stores it for later querying.

## Props

| Prop | Meaning | Notes |
|---|---|---|
| bounding_box | Optional geographic restriction for the AISStream subscription | Inline `min_latitude`, `max_latitude`, `min_longitude`, `max_longitude` values |

## Functionalities

### new

| Started by | Uses | Result |
|---|---|---|
| application startup | aisstream, event_sink, optional bounding_box | receiver is constructed |

Rules:
- Stores the provided role players and optional `bounding_box`.
- The `event_sink` is passed by shared reference because the same EventBufferContext may be shared with other contexts.
- Does not start the upstream subscription during construction.
- Call `start` to begin receiving messages.

| Given | When | Then |
|---|---|---|
| an aisstream collaborator, event sink, and optional bounding box are available | new is called | an AISStreamReceiverContext is returned without an active subscription |

### start

| Started by | Uses | Result |
|---|---|---|
| application runtime | aisstream, bounding_box | upstream position subscription begins |

Rules:
- Asks the collaborator playing the `aisstream` role to start a position subscription for this receiver.
- Uses the configured `bounding_box` when one is present.
- Uses the fixed positive filter `PositionReport` and `StandardClassBPositionReport`.
- Does not accept raw upstream message families outside those two message types.

| Given | When | Then |
|---|---|---|
| a configured receiver with a bounding box | start is called | the aisstream collaborator is asked to subscribe only to `PositionReport` and `StandardClassBPositionReport` for that region |

### receive_aisstream_message

| Started by | Uses | Result |
|---|---|---|
| aisstream collaborator | raw_message, event_sink | a mapped PositionEvent is delivered or the message is discarded |

Rules:
- Attempts to parse `raw_message` as an AISStreamPositionMessage.
- If parsing fails, discards the message silently and records a parse-failure count for diagnostics.
- If latitude or longitude is absent, discards the record silently.
- If latitude is `91.0` or longitude is `181.0`, discards the record silently because those are AIS invalid-coordinate sentinels.
- Maps latitude and longitude directly from the AISStream position message when valid.
- Uses the boundary record's `observed_at` as `occurred_at`.
- Sets `source` to `aisstream`.
- Uses the vessel name as `label` when it is present and non-blank.
- Falls back to the MMSI rendered as text when the vessel name is absent or blank.
- Passes the resulting PositionEvent to `event_sink.receive_event`.

| Given | When | Then |
|---|---|---|
| a subscribed position message with valid coordinates but no vessel name | receive_aisstream_message runs | a PositionEvent is delivered with source `aisstream` and the MMSI string as its label |

## Notes

- The positive filter is intentionally narrow: only `PositionReport` and `StandardClassBPositionReport` are included because they are sufficient to plot moving vessel dots on the map.
- AISStream wraps each payload in an envelope containing `MessageType`, `Message`, and provider-specific `MetaData`.
- The message-body field names differ by `MessageType`, but both included message families expose `UserID`, `Latitude`, `Longitude`, and `Timestamp`.
- The AIS `Timestamp` field in these message families is not a full UTC instant; it is only the reported second within the current UTC minute, so the receiver uses the boundary record's `observed_at` as the event timestamp.
- Vessel name may be absent because the included message families are position reports rather than static-data reports.
