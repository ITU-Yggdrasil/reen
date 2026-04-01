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

---

## Fields

icao, callsign, latitude, longitude, and timestamp are readable by AdsbReceiverContext
when mapping a record to a PositionEvent. altitude and squawk are not read during mapping.

- **icao**
  The 24-bit ICAO transponder address of the aircraft, expressed as a hexadecimal string.
  Uniquely identifies the aircraft's transponder. Always present.
  Read by AdsbReceiverContext when forming the label of the resulting PositionEvent if no
  callsign is available.

- **callsign**
  The flight identifier broadcast by the aircraft. May be absent if the transponder has
  not transmitted one, or if no ground-station update has been received recently.
  When present, used as the label of the resulting PositionEvent in preference to the ICAO.
  Read by AdsbReceiverContext when setting the label of the resulting PositionEvent.

- **latitude**
  The last decoded geographic latitude of the aircraft in decimal degrees.
  May be absent if no position message has been received recently.
  Read by AdsbReceiverContext when setting the latitude of the resulting PositionEvent.
  If absent, the record is silently discarded and no PositionEvent is produced.

- **longitude**
  The last decoded geographic longitude of the aircraft in decimal degrees.
  May be absent for the same reasons as latitude.
  Read by AdsbReceiverContext when setting the longitude of the resulting PositionEvent.
  If absent, the record is silently discarded and no PositionEvent is produced.

- **altitude**
  The barometric altitude of the aircraft in feet. May be expressed as a number or the
  special string "ground" when the aircraft is on the ground.
  Carried for informational completeness; not used in aggregation logic.

- **squawk**
  The four-digit octal transponder squawk code. Certain codes (e.g., 7500, 7600, 7700)
  indicate emergencies. Carried for informational completeness; not used in aggregation logic.

- **timestamp**
  The moment this position was last updated, stored as a UTC timestamp. The receiver is
  responsible for converting from the API's wire format (typically a Unix epoch integer)
  into a UTC timestamp before constructing the struct.
  Read by AdsbReceiverContext when setting the occurred_at of the resulting PositionEvent.

---

## Functionalities

- **new(icao, callsign, latitude, longitude, altitude, squawk, timestamp)**
  Constructs an AircraftState from the supplied field values after boundary parsing and
  timestamp normalization have already been performed.

- **icao()**
  Returns the aircraft ICAO transponder address.

- **callsign()**
  Returns the optional aircraft callsign.

- **latitude()**
  Returns the optional aircraft latitude.

- **longitude()**
  Returns the optional aircraft longitude.

- **altitude()**
  Returns the aircraft altitude value.

- **squawk()**
  Returns the optional transponder squawk code.

- **timestamp()**
  Returns the UTC timestamp associated with this aircraft position record.

---

## Access rules

AdsbReceiverContext reads AircraftState only through the explicit getter methods listed
above. The generated type must therefore expose readable methods for each field used by
the receiver, rather than relying on direct field access from outside the type.
