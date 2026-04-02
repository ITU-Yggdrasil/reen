# AISStream - Real-time AIS Vessel Tracking

## Description

AISStream provides a real-time global maritime vessel tracking stream via WebSocket.
Vessels broadcast AIS messages containing position, identity, speed, and heading. The API
delivers these as JSON messages over a persistent WebSocket connection authenticated by API
key.

This draft covers the AISStream boundary types needed to subscribe to vessel-position
traffic and parse the included position messages used by the world-data fixture.

## Authoritative Sources

- OpenAPI Local: ../../../shared/openapi/aisstream.yaml
- OpenAPI URL: https://raw.githubusercontent.com/aisstream/ais-message-models/master/type-definition.yaml
- Documentation URL: https://aisstream.io/documentation
- Schema Repository URL: https://github.com/aisstream/ais-message-models

## Consumed Surface

- WebSocket Streams: wss://stream.aisstream.io/v0/stream
- Schema Types: PositionReport, StandardClassBPositionReport
- Message Families: PositionReport, StandardClassBPositionReport

## Generated Data Specifications

- PositionReport
- StandardClassBPositionReport

## Notes

- The included message families are sufficient to plot moving vessel dots on the map.
- For both included message families, the generated model must make `UserID`, `Latitude`, `Longitude`, and `Timestamp` readable.
- `Timestamp` in these message bodies is only the AIS-reported second within the current UTC minute and must not be treated as a full UTC instant by downstream code.
- Fields defined as `object`, or otherwise lacking a stable representable shape in the target language, should be represented as text containing the serialized JSON value.
