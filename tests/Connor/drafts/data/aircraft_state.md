# Data: AircraftState

## Purpose

AircraftState is the raw record received from the ADS-B Exchange API before any mapping has taken place. It belongs only at the boundary of the AdsbReceiver context and must not flow further into the system. Once the receiver produces a PositionEvent from it, the AircraftState is discarded. Aircraft without a valid position fix (latitude or longitude absent) are silently dropped by the receiver.

## Attributes

- icao: ICAO transponder address expressed as hexadecimal text, used as fallback label when callsign is absent
- callsign: optional flight identifier broadcast by the aircraft, preferred label when present
- latitude: optional latitude in decimal degrees; if absent, no PositionEvent is produced
- longitude: optional longitude in decimal degrees; if absent, no PositionEvent is produced
- altitude: altitude in feet or the special value "ground", informational only
- squawk: optional four-digit octal squawk code, informational only
- timestamp: UTC timestamp for the observed position, converted from wire format by the receiver

## Invariants

## Operations

### new
Constructs an AircraftState from normalized boundary values.

## Scenarios

### aircraft with callsign
Given: icao "A1B2C3", callsign "UAL123", latitude 40.6, longitude -73.8, altitude 35000, and a UTC timestamp
When: new is called
Then: an AircraftState is returned; the receiver will use "UAL123" as the event label

### aircraft without callsign
Given: icao "A1B2C3", callsign absent, latitude 40.6, longitude -73.8
When: the receiver maps this record
Then: the ICAO hex code "A1B2C3" is used as the event label
