# Component: TransactionTable

## Purpose

Displays a filterable, chronologically ordered list of transactions for a single account. Combines a DateRangePicker for time-window filtering with a scrollable list of TransactionRows.

## Composition

- DateRangePicker: controls the visible time window
- TransactionRow: one instance per transaction within the filtered window

## Props

- account-id: the identifier of the account whose transactions are displayed
- transactions: the collection of transactions to render, pre-sorted by date descending

## Events

- filter-changed: fired when the DateRangePicker's range changes, carries the new start and end dates so the parent can fetch filtered data

## Invariants

- Transactions must display in reverse chronological order, newest first
- When the filtered set is empty, a contextual message must appear explaining that no transactions match the selected range
- The table must not re-sort or re-order while new data is loading

## Scenarios

### user narrows the date range
Given: a TransactionTable displaying 25 transactions for account FI42 1234 5600 0007 85
When: the user sets the DateRangePicker to 2026-04-10 through 2026-04-15
Then: filter-changed fires with start 2026-04-10 and end 2026-04-15, and the parent fetches the filtered transaction set

### no transactions match the selected range
Given: a TransactionTable for account FI42 1234 5600 0007 85 with the DateRangePicker set to 2026-01-01 through 2026-01-02
When: the filtered result contains zero transactions
Then: the table shows an empty-state message suggesting the user widen the date range
