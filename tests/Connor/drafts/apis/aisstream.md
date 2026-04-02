# AISStream - Real-time AIS Vessel Tracking

## Description

AISStream provides a real-time global maritime vessel tracking stream via WebSocket.
Vessels broadcast AIS messages containing position, identity, speed, and heading. The API
delivers these as JSON messages over a persistent WebSocket connection authenticated by API
key.

The generated API-facing types from this draft are used by both the AISStream integration
context that speaks the external API and the AISStreamReceiverContext that consumes
normalized boundary records derived from that API. This draft therefore defines one shared
model for websocket message envelopes, subscription requests, included AIS message payload
types, and the enum used to identify AIS message families.

## Authoritative Sources

- OpenAPI Local: ../../../shared/openapi/aisstream.yaml
- OpenAPI URL: https://raw.githubusercontent.com/aisstream/ais-message-models/master/type-definition.yaml
- Documentation URL: https://aisstream.io/documentation
- Schema Repository URL: https://github.com/aisstream/ais-message-models

## Consumed Surface

- WebSocket Streams: wss://stream.aisstream.io/v0/stream
- Schema Types: AisStreamMessage, SubscriptionMessage, PositionReport, StandardClassBPositionReport
- Message Families: PositionReport, StandardClassBPositionReport, UnknownMessage

## Generated Data Specifications

- AisMessageTypes
- AisStreamMessage
- SubscriptionMessage
- PositionReport
- StandardClassBPositionReport

## Notes

- `AisMessageTypes` is the shared enum that must be reused consistently across `AisStreamMessage.MessageType`, `SubscriptionMessage.filter_message_types`, and any helper logic that identifies AIS message families.
- The generated code must not create separate incompatible enums for the same message-family domain in different generated modules.
- `AisMessageTypes` must include at least `PositionReport`, `StandardClassBPositionReport`, and `UnknownMessage`.
- `AisStreamMessage` is the inbound websocket envelope from AISStream and its `MessageType` field must use the shared `AisMessageTypes` enum.
- `SubscriptionMessage` is the outbound subscription request and its message-family filter field must use the same `AisMessageTypes` enum.
- For both included message families, the generated model must make `UserID`, `Latitude`, `Longitude`, and `Timestamp` readable.
- `Timestamp` in the included AIS message bodies is only the AIS-reported second within the current UTC minute and must not be treated as a full UTC instant by downstream code.
- Fields defined as `object`, or otherwise lacking a stable representable shape in the target language, should be represented as text containing the serialized JSON value.
