# Data: Transaction

## Purpose
Represents a single financial transaction on an account, recording the movement of money with its dates, counterparty, and processing status. Transactions are the line items visible in account statements and transaction history.

## Attributes
- booking date: the date on which the transaction was recorded in the bank's books, a calendar date
- value date: the date on which the transaction affects the account's available balance, a calendar date
- counterparty: the name or description of the other party in the transaction, a text value
- amount: the monetary value of the transaction, a Money value (positive for credits, negative for debits)
- description: a human-readable narrative describing the transaction, a text value
- status: the processing state — one of "booked", "pending", or "rejected"
- account reference: a reference to the Account this transaction belongs to

## Invariants
- Value date must not be earlier than booking date by more than two business days
- Status must be one of the recognised processing states
- Every transaction must reference exactly one account
- The amount's currency must match the referenced account's currency

## Scenarios
### A booked credit transaction
Given: booking date 2026-03-15, value date 2026-03-15, counterparty "Acme Corp", amount of 50000 in DKK, description "Salary March", status "booked", account reference "ACC-001"
When: a Transaction is created
Then: the Transaction is accepted with all attributes set

### A pending debit transaction
Given: booking date 2026-03-16, value date 2026-03-17, counterparty "Electric Company", amount of -1200 in DKK, description "Electricity bill", status "pending", account reference "ACC-001"
When: a Transaction is created
Then: the Transaction is accepted with status "pending"

### Invalid status is rejected
Given: booking date 2026-03-15, value date 2026-03-15, counterparty "Test", amount of 1000 in DKK, description "Test", status "completed", account reference "ACC-001"
When: a Transaction is created
Then: the creation is rejected because "completed" is not a recognised status

### Value date far before booking date is rejected
Given: booking date 2026-03-15, value date 2026-03-10, counterparty "Test", amount of 1000 in DKK, description "Test", status "booked", account reference "ACC-001"
When: a Transaction is created
Then: the creation is rejected because the value date precedes the booking date by more than two business days
