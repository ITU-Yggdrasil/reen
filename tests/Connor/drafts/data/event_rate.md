# EventRate

## Description

EventRate expresses how frequently PositionEvents are arriving within a GeographicCell,
stated as a number of events per minute.

It is derived from an EventCount: the count is divided by the length of the time window
expressed in minutes. Like EventCount, it is a snapshot value computed on demand.

A cell with no recent events has a rate of zero. Rate is never negative.

---

## Fields

All fields are private. Collaborators read them only through the getter methods listed in
`Functionalities`.

- **cell**
  The geographic region this rate applies to.
  Read by MetricsContext when generating Prometheus labels and JSON cell boundary fields.

- **events_per_minute**
  The average number of events per minute observed in this cell over the time window.
  Computed as the event count divided by the window's minutes value.
  Represented as a 64-bit floating-point value (`f64`).
  No rounding is applied when constructing EventRate; the stored value is the direct
  result of the division expressed in `f64`.
  EventRate itself does not quantise, truncate, or format the value for output.
  Read by MetricsContext when setting the gauge value and the JSON rate field.

- **window**
  The time window from which the rate was derived.
  Read by MetricsContext if it needs to label or annotate the time window in output.

---

## Functionalities

- **new(cell, events_per_minute, window)**
  Constructs an EventRate from the supplied field values.

- **cell()**
  Returns the geographic cell this rate applies to.

- **events_per_minute()**
  Returns the computed average event rate.

- **window()**
  Returns the time window from which the rate was derived.

---

## Numeric rules

- `events_per_minute` is an `f64`.
- The value is computed as `event_count / window_minutes` using floating-point division.
- `window_minutes` must be greater than or equal to `1`; therefore division by zero does
  not occur in a valid EventRate.
- A zero-count cell has `events_per_minute = 0.0`.
- `events_per_minute` must be finite.
- Negative values, `NaN`, and positive or negative infinity are invalid and must not be constructed.
- EventRate defines no presentation rounding. Any rounding or string formatting for
  wire output is the responsibility of the serialising context, not EventRate itself.
