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

- User opens account detail: the table loads with the default date range and shows the most recent transactions
- User narrows the date range: filter-changed fires, the parent fetches filtered data, and the table re-renders with the reduced set
- No transactions exist in the selected range: the table shows an empty-state message suggesting the user widen the date range
