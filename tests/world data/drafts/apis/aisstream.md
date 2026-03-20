# AISStream — Real-time AIS Vessel Tracking
 
## Description
 
AISStream provides a real-time global maritime vessel tracking stream via WebSocket. Vessels broadcast AIS (Automatic Identification System) messages containing position, identity, speed, and heading. The API delivers these as JSON messages over a persistent WebSocket connection, authenticated by API key. The service is currently in beta and carries no uptime SLA.
 
## API Specification
 
- https://raw.githubusercontent.com/aisstream/ais-message-models/master/type-definition.yaml
 
## Documentation
 
- https://aisstream.io/documentation

## Included data types 

- PositionReport
- StandardClassBPositionReport

## Notes


Fields/properties that are defined as `object` or for other reasons don't have a type that can represented by the language should be defined as a text property and have the serialized value i.e. json as the value