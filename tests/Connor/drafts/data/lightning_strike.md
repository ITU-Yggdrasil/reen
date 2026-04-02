# LightningStrike

## Description

LightningStrike is the raw record as received from the Blitzortung websocket before any
mapping has taken place. It belongs only at the boundary of the LightningReceiverContext
and must not flow further into the system.

The LightningReceiverContext is solely responsible for reading this record and producing
a PositionEvent from it. Once that mapping is done, the LightningStrike is discarded.

## Fields

| Field | Meaning | Accessible | Notes |
|---|---|---|---|
| latitude | Geographic latitude of the detected strike | X | Used for PositionEvent latitude |
| longitude | Geographic longitude of the detected strike | X | Used for PositionEvent longitude |
| timestamp | UTC timestamp of the strike | X | Receiver parses from raw wire format |
| station_count | Optional number of contributing stations |  | Diagnostic only |

## Functionalities

- **new(latitude, longitude, timestamp, station_count)** Constructs a LightningStrike from the provided field values.
