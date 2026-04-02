# LightningReceiverContext

## Purpose

LightningReceiverContext is the boundary adapter for the Blitzortung websocket feed. It
owns the connection to the Blitzortung network, keeps that connection alive, and
translates each incoming LightningStrike into a PositionEvent before forwarding it into
the wider system.

This context fulfills the EventSource role in the system. Any context that needs to
receive events must only know about the EventSource role and must have no knowledge that
the underlying feed is Blitzortung, websocket-based, or lightning-specific. Swapping this
context out for another receiver, or running multiple receivers side by side, must require
no changes to downstream contexts.

## Role Players

| Role player | Why involved | Expected behaviour |
|---|---|---|
| event_sink | Receives produced PositionEvents | Accepts each successfully mapped lightning event |

## Role Methods

### event_sink

- **receive_event(event)** Accepts a single PositionEvent and stores it for later querying.

## Props

| Prop | Meaning | Notes |
|---|---|---|
| websocket_url | URL of the Blitzortung websocket endpoint | Connected when the receiver starts |
| reconnect_delay | How long to wait before reconnecting after a dropped connection | Configured as a duration value |

## Functionalities

### new

| Started by | Uses | Result |
|---|---|---|
| application startup | event_sink, websocket_url, reconnect_delay | receiver is constructed |

Rules:
- Stores `event_sink`, `websocket_url`, and `reconnect_delay`.
- The `event_sink` is passed by shared reference because the same EventBufferContext may be shared with other contexts.
- Does not open the websocket connection during construction.
- Call `start` to begin receiving strike messages.

| Given | When | Then |
|---|---|---|
| an event sink and websocket configuration are available | new is called | a LightningReceiverContext is returned without an open connection |

### start

| Started by | Uses | Result |
|---|---|---|
| application runtime | websocket_url | websocket receive loop begins |

Rules:
- Opens the websocket connection to the configured URL.
- Begins reading messages continuously in the background.
- Calls `on_connection_established` after a successful websocket handshake.
- Sends the required Blitzortung subscription frame `{"a":111}` after the connection is established.
- Continues processing messages until the connection is lost.

| Given | When | Then |
|---|---|---|
| a reachable Blitzortung endpoint | start is called | the receiver connects and sends the subscription frame needed to receive strike traffic |

### on_message_received

| Started by | Uses | Result |
|---|---|---|
| websocket receive loop | raw_message, event_sink | a mapped PositionEvent is delivered or the message is discarded |

Rules:
- Normalizes the incoming payload before parsing.
- If the payload is already valid JSON, uses it unchanged.
- Otherwise, if the payload matches the Blitzortung obfuscated websocket format, decodes it into JSON first.
- Attempts to parse the normalized JSON as a LightningStrike.
- If parsing succeeds, maps latitude and longitude directly from the strike.
- Uses the strike timestamp as `occurred_at`.
- Sets `source` to `lightning`.
- Sets `label` to the literal value `lightning`.
- Passes the resulting PositionEvent to `event_sink.receive_event`.
- If parsing fails, discards the message, increments a parse-failure count, and logs the parse error together with the payload used for parsing.

| Given | When | Then |
|---|---|---|
| an obfuscated Blitzortung payload containing a valid strike | on_message_received runs | the payload is decoded, parsed, and a PositionEvent with source `lightning` is delivered |

### on_connection_lost

| Started by | Uses | Result |
|---|---|---|
| websocket receive loop | reconnect_delay | reconnect attempt is deferred |

Rules:
- Runs when the websocket connection drops.
- Waits for the configured `reconnect_delay`.
- Allows the outer receive loop to establish a fresh websocket connection.
- Retains the same `event_sink` and configuration across reconnects.

| Given | When | Then |
|---|---|---|
| the websocket connection drops | on_connection_lost runs | a reconnect attempt is made after `reconnect_delay` has elapsed |

### on_connection_established

| Started by | Uses | Result |
|---|---|---|
| websocket handshake | active connection | successful-connection hook runs |

Rules:
- Runs immediately after the websocket handshake completes successfully.
- Serves as the successful-connection hook for the receiver lifecycle.
- Does not reset retained configuration or event sink state.
- Does not track reconnect back-off state.

| Given | When | Then |
|---|---|---|
| a websocket handshake completes successfully | on_connection_established runs | the receiver proceeds with its active connected lifecycle |

## Notes

- Binary websocket frames are first interpreted as UTF-8 text; if UTF-8 decoding fails, the parse-failure count increments and a decode failure is logged.
- For text frames, a normalization failure is not counted separately; the original text is still passed into strike parsing, and any resulting parse failure increments the parse-failure count by one.
- When `on_message_received` logs a parse failure, it logs the normalized JSON string when normalization succeeded and otherwise logs the original raw text payload.
- Blitzortung obfuscated decoding reads the payload as Unicode scalar values, emits the first character unchanged, maintains a string dictionary, and reconstructs later entries either directly from character values below `256` or from dictionary indices offset by `256`.
- The strike parser accepts latitude from `lat` or `latitude`, longitude from `lon`, `lng`, or `longitude`, timestamp from `time`, `timestamp`, or `ts`, numeric fields as either JSON numbers or numeric strings, and timestamps at second, millisecond, microsecond, or nanosecond precision.
