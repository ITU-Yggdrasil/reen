# FlightPosition

## Description

FlightPosition is the raw record as received from the OpenSky Network API before any
mapping has taken place. It belongs only at the boundary of the FlightReceiverContext
and must not flow further into the system.

The FlightReceiverContext is solely responsible for reading this record and producing
a PositionEvent from it. Once that mapping is done, the FlightPosition is discarded.

Flights without a valid position fix (latitude or longitude absent) must be silently
dropped by the receiver and must not produce a PositionEvent.

## Fields

| Field | Meaning | Accessible | Notes |
|---|---|---|---|
| callsign | Optional identifying callsign of the aircraft | X | Used as label when present |
| latitude | Optional last known latitude in decimal degrees | X | If absent, no PositionEvent is produced |
| longitude | Optional last known longitude in decimal degrees | X | If absent, no PositionEvent is produced |
| altitude | Optional altitude in metres above sea level |  | Informational only |
| timestamp | UTC observation timestamp | X | Receiver converts from OpenSky wire format |

## Functionalities

- **new(callsign, latitude, longitude, altitude, timestamp)** Constructs a FlightPosition from the supplied field values.
