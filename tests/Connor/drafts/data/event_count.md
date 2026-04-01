# EventCount

## Description

EventCount records how many PositionEvents were observed within a specific GeographicCell
over a specific TimeWindow.

It is a snapshot value — it reflects the state of the event buffer at the moment it was
computed. Two EventCounts for the same cell computed at different moments may differ.

---

## Fields

All fields are private. Collaborators read them only through the getter methods listed in
`Functionalities`.

- **cell**
  The geographic region this count applies to.
  Read by AggregationContext when constructing the corresponding EventRate.

- **count**
  The total number of PositionEvents whose coordinates fell within the cell and whose
  occurred_at timestamp fell within the relevant time window.
  Read by AggregationContext when computing the rate by dividing by the window's minutes value.

- **window**
  The time window over which the count was computed.
  Read by AggregationContext to carry the window forward into each EventRate.

---

## Functionalities

- **new(cell, count, window)**
  Constructs an EventCount from the supplied field values.

- **cell()**
  Returns the geographic cell this count applies to.

- **count()**
  Returns the number of matching events.

- **window()**
  Returns the time window over which the count was computed.

---

## Numeric rules

- `count` is a signed 32-bit integer (`i32`) in storage and on the wire.
- `count` represents a cardinality and therefore must be greater than or equal to `0`.
- Fractional counts are invalid and must not be constructed.
- AggregationContext increments counts in whole-event units only; each matching
  PositionEvent contributes exactly `1` to the total for its cell.
