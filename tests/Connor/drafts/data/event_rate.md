# Data: EventRate

## Purpose

EventRate expresses how frequently PositionEvents are arriving within a GeographicCell, stated as a number of events per minute. It is derived from an EventCount by dividing the count by the TimeWindow length in minutes. A cell with no recent events has a rate of zero.

## Attributes

- cell: the GeographicCell the rate applies to
- events_per_minute: average events per minute for the cell, as a non-negative finite decimal number
- window: the TimeWindow the rate was derived from

## Invariants

- events_per_minute must be finite and non-negative.
- Negative values, not-a-number, and infinity are invalid.
- The rate is computed as event count divided by window minutes using decimal division.
- Because the window is always at least one minute, division by zero does not occur.
- EventRate defines no presentation rounding; formatting is the responsibility of the serialising context.

## Operations

### new
Constructs an EventRate from the cell, events_per_minute, and window. Returns an error if the value is not finite or is negative.

## Scenarios

### rate from count and window
Given: an EventCount of 10 over a five-minute window
When: the rate is derived
Then: events_per_minute is 2.0

### zero-count cell
Given: an EventCount of 0
When: the rate is derived
Then: events_per_minute is 0.0
