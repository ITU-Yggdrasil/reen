# Component: DateRangePicker

## Purpose

Lets the user select a start date and an end date to define a time window, typically for filtering transaction history. Enforces that the start never exceeds the end and that the range stays within a permitted maximum span.

## Props

- start: the currently selected start date, may be empty
- end: the currently selected end date, may be empty
- max-span: the maximum number of days the range may cover, optional
- earliest: the earliest selectable date, optional
- latest: the latest selectable date, defaults to today

## Events

- range-changed: fired when either date changes, carries the new start and end values

## Invariants

- start must never be later than end
- If max-span is set, the distance between start and end must not exceed it
- Dates outside the earliest-to-latest window must be unselectable

## Scenarios

### user selects a valid date range
Given: a DateRangePicker with no dates selected and max-span of 90 days
When: the user selects start date 2026-04-01 and end date 2026-04-15
Then: range-changed fires with start 2026-04-01 and end 2026-04-15

### end date exceeds max span and start adjusts
Given: a DateRangePicker with start 2026-01-01 and max-span of 30 days
When: the user selects end date 2026-04-15
Then: the start date adjusts to 2026-03-16 and range-changed fires with both new values
