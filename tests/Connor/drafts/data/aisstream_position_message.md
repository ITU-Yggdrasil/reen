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

## Fields

| Field | Meaning | Accessible | Notes |
|---|---|---|---|
| message_type | AISStream message family that produced this record | X | Allowed values are `PositionReport` and `StandardClassBPositionReport` |
| mmsi | Vessel MMSI taken from upstream `UserID` | X | Used as fallback label |
| vessel_name | Optional vessel name from metadata | X | Preferred label when present |
| latitude | Optional normalized latitude in decimal degrees | X | Invalid default is `91.0` |
| longitude | Optional normalized longitude in decimal degrees | X | Invalid default is `181.0` |
| observed_at | Full UTC instant for the message observation | X | Used as PositionEvent timestamp |
| metadata_json | Optional serialized `MetaData` payload | X | Stored as text because upstream shape is not stable |

## Construction Rules

- Only `PositionReport` and `StandardClassBPositionReport` may be represented by this type.
- The record is normalized into one consistent shape before the receiver reads it.
- `observed_at` is a full UTC timestamp.
- Any provider metadata preserved on the record is serialized as text.

## Functionalities

- **new(message_type, mmsi, vessel_name, latitude, longitude, observed_at, metadata_json)** Constructs an AISStreamPositionMessage from normalized boundary values.
