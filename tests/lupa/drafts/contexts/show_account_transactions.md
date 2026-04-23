# Context: ShowAccountTransactions

## Purpose
Retrieves transactions for a specific account from Nordea's Open Banking API, filtered by date range. The customer must hold the transactions-read scope and own the account.

## Role Players
- authenticated_customer: AuthenticatedCustomer — the verified customer identity
- account: Account — the account whose transactions are requested
- bank: @nordea-openbanking — the external API returning transaction data
- filter: date range boundaries for the transaction query
- audit_sink: AuditSink — receives an audit record of the query

## Props
- required_scope: transactions-read

## Role Methods
### authenticated_customer
- verify_scope: confirms the customer's session grants the transactions-read scope
- verify_ownership: confirms the requested account belongs to this customer

### bank
- fetch_transactions: calls the Nordea transactions endpoint for the given account and date range, returning the list of transactions

### audit_sink
- record: appends an audit entry describing the transaction query and its outcome

## Functionalities
### show
The authenticated_customer's verify_scope confirms the transactions-read scope. The authenticated_customer's verify_ownership confirms the account belongs to the customer. The bank's fetch_transactions retrieves transactions within the filter's date range. The audit_sink's record logs the query. If the scope is missing or the account does not belong to the customer, the query is refused.

## Scenarios
### customer views transactions for own account within date range
Given: authenticated customer C-9281 holds scope transactions-read and owns account FI42 1234 5600 0007 85, which has three transactions between 2026-04-01 and 2026-04-15
When: show is invoked with account FI42 1234 5600 0007 85 and filter from 2026-04-01 to 2026-04-15
Then: three transactions are returned, and an audit entry records "showed transactions: 3 entries for account ending 0785" for customer C-9281

### customer attempts to view transactions for account they do not own
Given: authenticated customer C-9281 holds scope transactions-read, but account FI42 9999 0000 0001 00 belongs to customer C-6633
When: show is invoked with account FI42 9999 0000 0001 00
Then: the query is refused with reason "account does not belong to customer", no bank call is made, and an audit entry records "transaction query denied: ownership mismatch" for customer C-9281

### customer without transactions-read scope is refused
Given: authenticated customer C-4455 holds only scope accounts-read
When: show is invoked for account FI42 1234 5600 0007 85
Then: the query is refused with reason "missing required scope: transactions-read" and an audit entry records the denial
