# Projection: TransactionList

## Purpose

Given a list of transactions and a filter, produces the filtered-and-sorted list for rendering. Pure function of inputs — the filter logic is deterministic; no context coordination.

## Role Players

- transactions: Transaction — provides the transactions the projection filters and sorts

## Role Methods

### transactions
- read_all: provides the transactions to be filtered, sorted, and returned

## Functionalities

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

### filter by amount range
Given: transactions with amounts 50, 150, 200, 250, 300, 500
When: by_amount_range is queried with minimum 100 and maximum 250
Then: returns the three transactions with amounts 150, 200, and 250

### amount-range bounds are inclusive on both ends
Given: transactions with amounts 100, 250, 500, 1000
When: by_amount_range is queried with minimum 100 and maximum 500
Then: returns the three transactions with amounts 100, 250, and 500
