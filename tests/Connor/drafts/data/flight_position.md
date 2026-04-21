# Data: FlightPosition

## Purpose

FlightPosition is the raw record received from the OpenSky Network API before any mapping has taken place. It belongs only at the boundary of the FlightReceiver context and must not flow further into the system. Once the receiver produces a PositionEvent from it, the FlightPosition is discarded. Flights without a valid position fix (latitude or longitude absent) are silently dropped by the receiver.

## Attributes

- callsign: optional identifying callsign of the aircraft, used as the event label when present
- latitude: optional last known latitude in decimal degrees; if absent, no PositionEvent is produced
- longitude: optional last known longitude in decimal degrees; if absent, no PositionEvent is produced
- altitude: optional altitude in metres above sea level, informational only
- timestamp: UTC observation timestamp, converted from the OpenSky wire format by the receiver

## Invariants

## Operations

### new
Constructs a FlightPosition from the supplied field values.

## Scenarios

### create a flight record with position
Given: callsign "SAS1234", latitude 55.6, longitude 12.6, altitude 10000, and a UTC timestamp
When: new is called
Then: a FlightPosition is returned with those values

### missing coordinates
Given: callsign "SAS1234", latitude absent, longitude absent
When: the receiver processes this record
Then: no PositionEvent is produced
