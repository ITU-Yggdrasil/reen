# EventCount

## Description

EventCount records how many PositionEvents were observed within a specific GeographicCell
over a specific TimeWindow.

It is a snapshot value — it reflects the state of the event buffer at the moment it was
computed. Two EventCounts for the same cell computed at different moments may differ.

## Fields

| Field | Meaning | Accessible | Notes |
|---|---|---|---|
| cell | Geographic region the count applies to | X | Used to build EventRate |
| count | Number of matching PositionEvents in the cell | X | Whole-event cardinality |
| window | Time window the count was computed over | X | Carried into EventRate |

## Functionalities

- **new(cell, count, window)** Constructs an EventCount from the supplied field values.

## Rules

- `count` is a signed 32-bit integer (`i32`) in storage and on the wire.
- `count` represents a cardinality and therefore must be greater than or equal to `0`.
- Fractional counts are invalid and must not be constructed.
- AggregationContext increments counts in whole-event units only; each matching
  PositionEvent contributes exactly `1` to the total for its cell.
