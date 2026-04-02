# TimeWindow

## Description

TimeWindow defines the span of time that counts as "recent" for the purpose of event
aggregation and buffer eviction.

Any PositionEvent whose occurred_at timestamp is older than the current time minus the
window duration is considered stale. The EventBufferContext uses this boundary to evict
old events; the AggregationContext uses it to bound the population it counts over.

The default duration is five minutes. It is configurable at startup.

## Fields

| Field | Meaning | Accessible | Notes |
|---|---|---|---|
| duration | Human-readable label such as `5 minutes` |  | Display and logging only |
| minutes | Numeric duration in whole minutes | X | Machine-usable value used by buffer and aggregator |

## Functionalities

- **new(duration, minutes)** Constructs a TimeWindow from the human-readable label and machine-usable minute value.

## Rules

- `minutes` is a signed 32-bit integer (`i32`).
- `minutes` must be a whole number of minutes greater than or equal to `1`.
- `minutes = 0` is invalid and must not be constructed.
- Negative and fractional minute values are invalid and must not be constructed.
- The leading integer minute value encoded in `duration` must exactly equal `minutes`.
