# EventRate

## Description

EventRate expresses how frequently PositionEvents are arriving within a GeographicCell,
stated as a number of events per minute.

It is derived from an EventCount: the count is divided by the length of the time window
expressed in minutes. Like EventCount, it is a snapshot value computed on demand.

A cell with no recent events has a rate of zero. Rate is never negative.

## Fields

| Field | Meaning | Accessible | Notes |
|---|---|---|---|
| cell | Geographic region the rate applies to | X | Used in metrics labels and JSON output |
| events_per_minute | Average events per minute for the cell | X | Stored as `f64` without presentation rounding |
| window | Time window the rate was derived from | X | Carried alongside the computed rate |

## Functionalities

- **new(cell, events_per_minute, window)** Constructs an EventRate from the supplied field values.

## Rules

- `events_per_minute` is an `f64`.
- The value is computed as `event_count / window_minutes` using floating-point division.
- `window_minutes` must be greater than or equal to `1`; therefore division by zero does
  not occur in a valid EventRate.
- A zero-count cell has `events_per_minute = 0.0`.
- `events_per_minute` must be finite.
- Negative values, `NaN`, and positive or negative infinity are invalid and must not be constructed.
- EventRate defines no presentation rounding. Any rounding or string formatting for
  wire output is the responsibility of the serialising context, not EventRate itself.
