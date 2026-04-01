# AISStreamPositionMessage

## Description

AISStreamPositionMessage is the boundary-only record delivered from AISStream into
AISStreamReceiverContext after subscription filtering has already been applied.

It represents exactly one upstream AISStream message from the included message families:
- `PositionReport`
- `StandardClassBPositionReport`

This record must not flow further into the system. AISStreamReceiverContext is solely
responsible for reading it and producing a PositionEvent. Once that mapping is done, the
AISStreamPositionMessage is discarded.

The AISStream message envelope contains an untyped `MetaData` object and a typed message
body selected by `MessageType`. The receiver only relies on the normalized fields defined
here so that downstream code does not depend on provider-specific JSON layout details.

---

## Fields

`message_type`, `mmsi`, `vessel_name`, `latitude`, `longitude`, and `observed_at` are read
by AISStreamReceiverContext when mapping a record to a PositionEvent. `metadata_json` is
carried only for boundary completeness and diagnostics.

- **message_type**
  The AISStream message family that produced this record.
  Allowed values are `PositionReport` and `StandardClassBPositionReport`.
  Read by AISStreamReceiverContext only for validation and diagnostics.

- **mmsi**
  The vessel MMSI taken from the upstream `UserID` field.
  Always present.
  Read by AISStreamReceiverContext when forming the label of the resulting PositionEvent if
  no vessel name is available.

- **vessel_name**
  The vessel name when AISStream metadata makes one available.
  May be absent because the included message families are position reports rather than
  static-data reports.
  When present, used as the label of the resulting PositionEvent in preference to the MMSI.
  Read by AISStreamReceiverContext when setting the label of the resulting PositionEvent.

- **latitude**
  The vessel latitude in decimal degrees, normalized from the included AISStream message
  body.
  May be absent or set to the AIS default invalid value 91.0 degrees if no valid fix is
  available.
  Read by AISStreamReceiverContext when setting the latitude of the resulting PositionEvent.
  If absent or invalid, the record is silently discarded and no PositionEvent is produced.

- **longitude**
  The vessel longitude in decimal degrees, normalized from the included AISStream message
  body.
  May be absent or set to the AIS default invalid value 181.0 degrees if no valid fix is
  available.
  Read by AISStreamReceiverContext when setting the longitude of the resulting PositionEvent.
  If absent or invalid, the record is silently discarded and no PositionEvent is produced.

- **observed_at**
  The UTC instant that AISStreamContext observed or decoded this upstream message.
  This is the timestamp used when constructing PositionEvent.
  It must be supplied by the AISStream communication layer because the included AIS message
  bodies expose only an AIS `Timestamp` field representing the second within the current UTC
  minute, not a complete UTC instant.
  Read by AISStreamReceiverContext when setting `occurred_at` on the resulting PositionEvent.

- **metadata_json**
  An optional serialized copy of the upstream AISStream `MetaData` object.
  Stored as text because the provider schema leaves `MetaData` untyped.
  Not required for PositionEvent mapping once the normalized fields above have been filled.

---

## Construction rules

- Only `PositionReport` and `StandardClassBPositionReport` may be represented by this type.
- The record must already be normalized into one consistent shape before the receiver reads
  it.
- `observed_at` must be a full UTC timestamp.
- Any provider metadata preserved on the record must be serialized as text.

---

## Functionalities

- **new(message_type, mmsi, vessel_name, latitude, longitude, observed_at, metadata_json)**
  Constructs an AISStreamPositionMessage from the supplied normalized values.

- **message_type()**
  Returns the AISStream message family that produced this boundary record.

- **mmsi()**
  Returns the vessel MMSI as text.

- **vessel_name()**
  Returns the optional vessel name.

- **latitude()**
  Returns the optional vessel latitude.

- **longitude()**
  Returns the optional vessel longitude.

- **observed_at()**
  Returns the UTC instant used when constructing the downstream PositionEvent.

- **metadata_json()**
  Returns the optional serialized AISStream metadata payload.

---

## Access rules

AISStreamReceiverContext reads AISStreamPositionMessage only through the explicit getter
methods listed above. The generated type must therefore expose readable methods for the
fields used by the receiver rather than relying on direct field access from outside the type.
