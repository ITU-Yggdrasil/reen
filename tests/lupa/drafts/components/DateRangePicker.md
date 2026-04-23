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

- User selects a start date after the current end date: the end date clears and the user is prompted to pick a new end
- User selects an end date that would exceed max-span: the start date adjusts forward to satisfy the constraint and range-changed fires with both new values
- User clears both dates: range-changed fires with empty values, indicating no filter is applied
