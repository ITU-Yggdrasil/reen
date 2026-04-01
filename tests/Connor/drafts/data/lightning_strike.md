# LightningStrike

## Description

LightningStrike is the raw record as received from the Blitzortung websocket before any
mapping has taken place. It belongs only at the boundary of the LightningReceiverContext
and must not flow further into the system.

The LightningReceiverContext is solely responsible for reading this record and producing
a PositionEvent from it. Once that mapping is done, the LightningStrike is discarded.

---

## Fields

All fields are private. LightningReceiverContext reads latitude, longitude, and timestamp
only through the getter methods listed in `Functionalities`. station_count is not read
during mapping.

- **latitude**
  The geographic latitude of the detected strike, as reported by the Blitzortung network.
  Read by LightningReceiverContext when setting the latitude of the resulting PositionEvent.

- **longitude**
  The geographic longitude of the detected strike, as reported by the Blitzortung network.
  Read by LightningReceiverContext when setting the longitude of the resulting PositionEvent.

- **timestamp**
  The moment the strike was detected, stored as a UTC timestamp. The receiver is responsible
  for parsing the raw wire format (e.g. a nanosecond Unix integer) into a UTC timestamp before
  constructing the struct.
  Read by LightningReceiverContext when setting the occurred_at of the resulting PositionEvent.

- **station_count**
  An optional signal indicating how many receiving stations contributed to this observation.
  Higher values generally indicate greater positional confidence. May be absent for some
  messages. Used only for diagnostic purposes; aggregation logic must not vary based on it.

---

## Functionalities

- **new(latitude, longitude, timestamp, station_count)**
  Constructs a LightningStrike from the provided field values.

- **latitude()**
  Returns the strike latitude.

- **longitude()**
  Returns the strike longitude.

- **timestamp()**
  Returns the strike timestamp.
