# AISStream — Real-time AIS Vessel Tracking

## Description

AISStream provides a real-time global maritime vessel tracking stream via WebSocket.
Vessels broadcast AIS (Automatic Identification System) messages containing position,
identity, speed, and heading. The API delivers these as JSON messages over a persistent
WebSocket connection, authenticated by API key. The service is currently in beta and
carries no uptime SLA.

The generated API-facing types from this draft are used by both:
- the AISStream integration context that speaks the external API, and
- AISStreamReceiverContext, which consumes normalized boundary records derived from that API.

This draft must therefore define one consistent shared model for:
- websocket message envelopes,
- subscription requests,
- included AIS message payload types,
- and the enum used to identify AIS message families.

## API Specification

- https://raw.githubusercontent.com/aisstream/ais-message-models/master/type-definition.yaml

## Documentation

- https://aisstream.io/documentation

## Canonical shared types

The following generated types are part of the API surface and are expected to be reused
across the integration layer rather than regenerated as unrelated duplicate enums or
message-family identifiers.

- **AisMessageTypes**
  A single canonical enum representing AIS message families referenced anywhere in the
  AISStream integration.

- **AisStreamMessage**
  The websocket envelope received from AISStream.

- **SubscriptionMessage**
  The outbound subscription request sent to AISStream.

- **PositionReport**
  Included AIS message body type.

- **StandardClassBPositionReport**
  Included AIS message body type.

## Canonical enum: AisMessageTypes

`AisMessageTypes` is a shared enum type used consistently across:
- `AisStreamMessage.MessageType`,
- `SubscriptionMessage.filter_message_types`,
- and any helper or mapping code that identifies AIS message families.

The generated code must not create separate incompatible enums for the same conceptual
message-family domain in different generated modules.

At minimum, this enum must include:
- `PositionReport`
- `StandardClassBPositionReport`
- `UnknownMessage`

If the upstream schema defines more values, they may also be included, but these three
must exist and the same enum type must be reused everywhere message-family selection or
identification occurs.

## AisStreamMessage

This is the inbound websocket envelope from AISStream.

- **MessageType**
  Uses the canonical shared enum `AisMessageTypes`.

- **Message**
  Contains the typed message body selected by `MessageType`.
  For this project, only the included message families below need to be modeled in a
  strongly-typed way.

- **MetaData**
  Provider metadata object. When it cannot be represented as a stable typed structure, it
  should be represented as serialized text containing the JSON value.

## SubscriptionMessage

This is the outbound subscription request sent to AISStream when starting or refreshing a
position subscription.

It must include:
- the positive filter of message families to subscribe to,
- an optional geographic restriction via bounding box,
- and any other required subscription envelope fields from the AISStream API.

The message-family filter field must use the same canonical `AisMessageTypes` enum used by
`AisStreamMessage.MessageType`.

## Included message body types

The following AIS payload families must be modeled explicitly because Connor uses them for
position extraction and subscription filtering:

- **PositionReport**
- **StandardClassBPositionReport**

For both included message families, the generated model must make these fields readable:
- `UserID`
- `Latitude`
- `Longitude`
- `Timestamp`

`Timestamp` in these message bodies is only the AIS-reported second within the current UTC
minute. It is not a full UTC instant and must not be treated as one by downstream code.

## Notes

Fields/properties defined as `object`, or otherwise lacking a stable representable shape in
the target language, should be represented as text containing the serialized JSON value.
