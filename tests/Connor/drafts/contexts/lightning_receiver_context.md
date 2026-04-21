# Context: LightningReceiverContext

## Purpose

LightningReceiver is the boundary adapter for the Blitzortung websocket feed. It owns the connection to the Blitzortung network, keeps that connection alive, and translates each incoming LightningStrike into a PositionEvent before forwarding it into the wider system. This context fulfills the EventSource role — downstream contexts have no knowledge that the underlying feed is Blitzortung, websocket-based, or lightning-specific.

## Role Players

- event_sink: EventBufferContext — receives each successfully mapped lightning event as a PositionEvent

## Props

- websocket_url: URL of the Blitzortung websocket endpoint, connected when the receiver starts
- reconnect_delay: how long to wait before reconnecting after a dropped connection

## Role Methods

### event_sink
- receive_event: accepts a single PositionEvent and stores it for later querying

## Functionalities

### new
Stores the event sink, websocket URL, and reconnect delay. Does not open the websocket connection during construction.

### start
Opens the websocket connection to the configured URL and begins reading messages continuously in the background. After a successful handshake, sends the required Blitzortung subscription frame. Continues processing messages until the connection is lost.

### on_message_received
Normalizes the incoming payload before parsing. If the payload is already valid JSON, uses it unchanged. Otherwise, if the payload matches the Blitzortung obfuscated websocket format, decodes it into JSON first. Attempts to parse the normalized JSON as a LightningStrike. If parsing succeeds, maps latitude and longitude directly from the strike, uses the strike timestamp as occurred_at, sets source to "lightning" and label to "lightning", and delivers the resulting PositionEvent to the event sink. If parsing fails, discards the message, increments a parse-failure count, and logs the error together with the payload.

### on_connection_lost
Runs when the websocket connection drops. Waits for the configured reconnect delay, then allows the outer receive loop to establish a fresh connection. Retains the same event sink and configuration across reconnects.

### on_connection_established
Runs immediately after the websocket handshake completes successfully. Serves as the successful-connection hook for the receiver lifecycle. Does not reset retained configuration or event sink state.

## Scenarios

### obfuscated payload produces event
Given: an obfuscated Blitzortung payload containing a valid strike
When: on_message_received runs
Then: the payload is decoded, parsed, and a PositionEvent with source "lightning" is delivered

### reconnect after disconnect
Given: the websocket connection drops
When: on_connection_lost runs
Then: a reconnect attempt is made after the reconnect delay has elapsed

### unparseable message is discarded
Given: a payload that cannot be parsed as a LightningStrike
When: on_message_received runs
Then: the message is discarded and a parse-failure count is incremented
