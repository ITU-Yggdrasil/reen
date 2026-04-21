# AISStream

## Purpose

AISStream provides a real-time global maritime vessel tracking stream via WebSocket. Vessels broadcast AIS messages containing position, identity, speed, and heading. The service delivers these as structured messages over a persistent WebSocket connection authenticated by API key. This boundary covers the subscription and position-report message families needed to plot moving vessel dots on a map.

## Source

- OpenAPI Local: ./aisstream.yaml
- OpenAPI URL: https://raw.githubusercontent.com/aisstream/ais-message-models/master/type-definition.yaml
- Documentation URL: https://aisstream.io/documentation

## Operations

- PositionReport
- StandardClassBPositionReport

## Notes

- The stream uses WebSocket transport at wss://stream.aisstream.io/v0/stream
- Authentication is via API key sent in the subscription message body
- Timestamp in message bodies is only the AIS-reported second within the current UTC minute, not a full UTC instant
- Fields defined as object or lacking a stable shape should be represented as text containing serialized content
