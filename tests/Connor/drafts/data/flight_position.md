# FlightPosition

## Description

FlightPosition is the raw record as received from the OpenSky Network API before any
mapping has taken place. It belongs only at the boundary of the FlightReceiverContext
and must not flow further into the system.

The FlightReceiverContext is solely responsible for reading this record and producing
a PositionEvent from it. Once that mapping is done, the FlightPosition is discarded.

Flights without a valid position fix (latitude or longitude absent) must be silently
dropped by the receiver and must not produce a PositionEvent.

---

## Fields

All fields are private. FlightReceiverContext reads callsign, latitude, longitude, and
timestamp only through the getter methods listed in `Functionalities`. altitude is not
read during mapping.

- **callsign**
  The identifying callsign of the aircraft as reported by OpenSky. May be absent or
  blank for some transponders. When present, it is carried through as the label of the
  resulting PositionEvent.
  Read by FlightReceiverContext when setting the label of the resulting PositionEvent.

- **latitude**
  The last known geographic latitude of the aircraft in decimal degrees.
  May be absent if the transponder has not reported a valid fix.
  Read by FlightReceiverContext when setting the latitude of the resulting PositionEvent.
  If absent, the record is silently discarded and no PositionEvent is produced.

- **longitude**
  The last known geographic longitude of the aircraft in decimal degrees.
  May be absent if the transponder has not reported a valid fix.
  Read by FlightReceiverContext when setting the longitude of the resulting PositionEvent.
  If absent, the record is silently discarded and no PositionEvent is produced.

- **altitude**
  The barometric or geometric altitude of the aircraft in metres above sea level.
  Carried for informational completeness; not used by aggregation logic.

- **timestamp**
  The moment at which this position was last observed, stored as a UTC timestamp. The
  receiver is responsible for converting from the wire format reported by OpenSky into
  a UTC timestamp before constructing the struct.
  Read by FlightReceiverContext when setting the occurred_at of the resulting PositionEvent.

---

## Functionalities

- **new(callsign, latitude, longitude, altitude, timestamp)**
  Constructs a FlightPosition from the supplied field values.

- **callsign()**
  Returns the optional callsign.

- **latitude()**
  Returns the optional latitude.

- **longitude()**
  Returns the optional longitude.

- **timestamp()**
  Returns the UTC observation timestamp.
