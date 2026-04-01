# TimeWindow

## Description

TimeWindow defines the span of time that counts as "recent" for the purpose of event
aggregation and buffer eviction.

Any PositionEvent whose occurred_at timestamp is older than the current time minus the
window duration is considered stale. The EventBufferContext uses this boundary to evict
old events; the AggregationContext uses it to bound the population it counts over.

The default duration is five minutes. It is configurable at startup.

---

## Fields

- **duration**
  The length of the window expressed as a human-readable label — for example
  "5 minutes" or "10 minutes". Intended for display and logging purposes only.
  This field is not readable by collaborators; it exists solely for human consumption.

- **minutes**
  The same duration expressed as a plain number of minutes. This is the machine-usable
  form: the EventBufferContext uses it to compute the staleness boundary, and the
  AggregationContext uses it as the divisor when deriving event rates. The default value
  is five. Both fields must always agree — they represent the same duration in different
  forms. This field is private; collaborators read it only through the `minutes()`
  getter listed in `Functionalities`.

---

## Functionalities

- **new(duration, minutes)**
  Constructs a TimeWindow from the human-readable label and machine-usable minute value.

- **minutes()**
  Returns the numeric window length in minutes.

---

## Numeric rules

- `minutes` is a signed 32-bit integer (`i32`).
- `minutes` must be a whole number of minutes greater than or equal to `1`.
- `minutes = 0` is invalid and must not be constructed.
- Negative and fractional minute values are invalid and must not be constructed.
- The leading integer minute value encoded in `duration` must exactly equal `minutes`.
