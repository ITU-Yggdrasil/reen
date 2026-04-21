# Data: TimeWindow

## Purpose

TimeWindow defines the span of time that counts as "recent" for the purpose of event aggregation and buffer eviction. Any PositionEvent whose occurred_at timestamp is older than the current time minus the window duration is considered stale. The default duration is five minutes.

## Attributes

- duration: human-readable label such as "5 minutes", used for display and logging
- minutes: numeric duration in whole minutes, used by the buffer and aggregator

## Invariants

- The minutes value must be a whole number greater than or equal to 1.
- Zero, negative, and fractional minute values are invalid.
- The leading integer in the duration label must exactly equal the minutes value.

## Operations

### new
Constructs a TimeWindow from the human-readable label and the numeric minute value. Returns an error if the minutes value is invalid or if the duration label does not match.

## Scenarios

### valid five-minute window
Given: duration "5 minutes" and minutes 5
When: new is called
Then: a TimeWindow is returned

### zero minutes is rejected
Given: duration "0 minutes" and minutes 0
When: new is called
Then: an error is returned

### mismatched duration is rejected
Given: duration "10 minutes" and minutes 5
When: new is called
Then: an error is returned because the leading integer does not match
