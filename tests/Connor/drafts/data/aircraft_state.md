# AircraftState

## Description

AircraftState is the raw record as received from the ADS-B Exchange API before any mapping
has taken place. It belongs only at the boundary of the AdsbReceiverContext and must not
flow further into the system.

The AdsbReceiverContext is solely responsible for reading this record and producing a
PositionEvent from it. Once that mapping is done, the AircraftState is discarded.

ADS-B Exchange aggregates raw ADS-B transponder broadcasts from a global network of
volunteer receivers. Coverage is densest over Europe and North America. Aircraft without
ADS-B equipment or with transponders switched off will not appear in the feed.

Aircraft without a valid position fix (latitude or longitude absent) must be silently
dropped by the receiver and must not produce a PositionEvent.

## Fields

| Field | Meaning | Accessible | Notes |
|---|---|---|---|
| icao | 24-bit ICAO transponder address expressed as hexadecimal text | X | Used as fallback label when callsign is absent |
| callsign | Optional flight identifier broadcast by the aircraft | X | Preferred label when present |
| latitude | Optional latitude in decimal degrees | X | If absent, no PositionEvent is produced |
| longitude | Optional longitude in decimal degrees | X | If absent, no PositionEvent is produced |
| altitude | Altitude in feet or the special value `ground` | X | Informational only |
| squawk | Optional four-digit octal squawk code | X | Informational only |
| timestamp | UTC timestamp for the observed position | X | Receiver converts from wire format before construction |

## Functionalities

- **new(icao, callsign, latitude, longitude, altitude, squawk, timestamp)** Constructs an AircraftState from normalized boundary values.
