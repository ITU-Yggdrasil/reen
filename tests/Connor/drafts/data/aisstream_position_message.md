# Data: AISStreamPositionMessage

## Purpose

AISStreamPositionMessage is the boundary-only record delivered from the AISStream transport into the AISStreamReceiver context after subscription filtering has already been applied. It represents exactly one upstream message from the included message families: PositionReport and StandardClassBPositionReport. Once the receiver produces a PositionEvent from it, the record is discarded.

## Attributes

- message_type: the AISStream message family that produced this record; allowed values are "PositionReport" and "StandardClassBPositionReport"
- mmsi: vessel MMSI taken from the upstream user identifier, used as fallback label
- vessel_name: optional vessel name from metadata, preferred label when present
- latitude: optional normalized latitude in decimal degrees; the invalid sentinel value 91.0 indicates absence
- longitude: optional normalized longitude in decimal degrees; the invalid sentinel value 181.0 indicates absence
- observed_at: full UTC instant for the message observation, used as the PositionEvent timestamp
- metadata_json: optional serialized metadata payload stored as text because the upstream shape is not stable

## Invariants

- Only PositionReport and StandardClassBPositionReport message families may be represented.
- The record is normalized into one consistent shape before the receiver reads it.
- observed_at is a full UTC timestamp (the AIS Timestamp field in these message families is only the reported second within the current UTC minute and must not be used as the event timestamp).

## Operations

### new
Constructs an AISStreamPositionMessage from normalized boundary values.

## Scenarios

### valid position message
Given: message_type "PositionReport", mmsi "123456789", vessel_name "CARGO SHIP", latitude 51.5, longitude -0.1, observed_at as a UTC timestamp
When: new is called
Then: an AISStreamPositionMessage is returned

### invalid latitude sentinel
Given: latitude 91.0
When: the receiver processes this record
Then: the record is discarded because 91.0 is the AIS invalid-coordinate sentinel
