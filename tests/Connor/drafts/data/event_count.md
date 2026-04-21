# Data: EventCount

## Purpose

EventCount records how many PositionEvents were observed within a specific GeographicCell over a specific TimeWindow. It is a snapshot value — it reflects the state of the event buffer at the moment it was computed.

## Attributes

- cell: the GeographicCell the count applies to
- count: the number of matching PositionEvents in the cell, as a non-negative whole number
- window: the TimeWindow the count was computed over

## Invariants

- The count must be greater than or equal to zero.
- Fractional counts are invalid.
- Each matching PositionEvent contributes exactly one to the total for its cell.

## Operations

### new
Constructs an EventCount from the cell, count, and window.

## Scenarios

### cell with three events
Given: a GeographicCell, a count of 3, and a five-minute TimeWindow
When: new is called
Then: an EventCount with count 3 is returned

### empty cell has zero count
Given: a GeographicCell with no matching events
When: the count is computed
Then: the count is zero
