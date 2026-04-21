# Context: AISStreamReceiverContext

## Purpose

AISStreamReceiver turns raw AISStream position messages into feed-agnostic PositionEvents for the rest of the system. It does not own the websocket connection, authentication, or reconnect behaviour — those concerns belong to the collaborator playing the aisstream role. This context asks that collaborator for a subscription limited to the included position-message types, receives the resulting boundary records, and forwards mapped PositionEvents into the wider system.

## Role Players

- aisstream: AISStreamTransport — owns the upstream communication and subscription lifecycle, delivering only subscribed messages
- event_sink: EventBufferContext — receives each successfully mapped vessel event as a PositionEvent

## Props

- bounding_box: optional geographic restriction for the AISStream subscription (min_latitude, max_latitude, min_longitude, max_longitude)

## Role Methods

### aisstream
- start_position_subscription: starts or refreshes an AISStream subscription and delivers matching raw messages; the positive filter for this receiver is exactly PositionReport and StandardClassBPositionReport

### event_sink
- receive_event: accepts a single PositionEvent and stores it for later querying

## Functionalities

### new
Stores the aisstream collaborator, event sink, and optional bounding box. Does not start the upstream subscription during construction.

### start
Asks the aisstream collaborator to start a position subscription using the configured bounding box and the fixed positive filter of PositionReport and StandardClassBPositionReport. Does not accept upstream message families outside those two types.

### receive_aisstream_message
Attempts to parse the raw message as an AISStreamPositionMessage. If parsing fails, discards the message silently and records a parse-failure count for diagnostics. If latitude or longitude is absent, discards the record silently. If latitude is 91.0 or longitude is 181.0, discards the record silently because those are AIS invalid-coordinate sentinels. Maps latitude and longitude directly from the position message when valid. Uses the boundary record's observed_at as occurred_at. Sets source to "aisstream". Uses the vessel name as label when present and non-blank. Falls back to the MMSI rendered as text when the vessel name is absent or blank. Delivers the resulting PositionEvent to the event sink.

## Scenarios

### valid message without vessel name
Given: a subscribed position message with valid coordinates but no vessel name
When: receive_aisstream_message runs
Then: a PositionEvent is delivered with source "aisstream" and the MMSI text as its label

### invalid coordinate sentinel is discarded
Given: a message with latitude 91.0
When: receive_aisstream_message runs
Then: the record is discarded

### subscription uses narrow filter
Given: a configured receiver with a bounding box
When: start is called
Then: the aisstream collaborator is asked to subscribe only to PositionReport and StandardClassBPositionReport for that region
