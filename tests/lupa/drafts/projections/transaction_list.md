# Projection: TransactionList

## Purpose

Given a list of transactions and a filter, produces the filtered-and-sorted list for rendering. Pure function of inputs — the filter logic is deterministic; no context coordination.

## Source
- Transaction

## Queries
### filtered
Returns the transactions that match the given date range (start and end, both inclusive), sorted by booking date with newest first. Transactions outside the range are excluded. If no date range is provided, all transactions are returned.

### by_counterparty
Returns transactions whose counterparty name contains the given search text, case-insensitive.

### by_amount_range
Returns transactions whose amount falls within the given minimum and maximum bounds, inclusive.

## Scenarios
### filter by date range
Given: ten transactions spanning the last thirty days
When: filtered is queried with a range covering the last seven days
Then: only transactions booked within the last seven days are returned, newest first

### no filter returns all
Given: five transactions
When: filtered is queried with no date range
Then: all five transactions are returned, newest first

### counterparty search
Given: transactions with counterparties "Alice", "Bob", "Alice Corp"
When: by_counterparty is queried with "alice"
Then: returns the two transactions with "Alice" and "Alice Corp"
