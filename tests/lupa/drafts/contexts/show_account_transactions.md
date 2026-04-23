# Context: ShowAccountTransactions

## Purpose
Retrieves transactions for a specific account from the Open Banking integration, filtered by date range. The customer must hold the transactions-read scope and own the account.

## Role Players
- authenticated_customer: AuthenticatedCustomer — the verified customer identity
- account: Account — the account whose transactions are requested

## Props
- required_scope: transactions-read
- date_from: the start date of the transaction query range
- date_to: the end date of the transaction query range

## Role Methods
### account
- provide_identifier: supplies the account identifier for the transaction fetch

### authenticated_customer
- verify_scope: confirms the customer's session grants the transactions-read scope
- verify_ownership: confirms the requested account belongs to this customer

## Functionalities
### show
The authenticated_customer's verify_scope confirms the transactions-read scope. The authenticated_customer's verify_ownership confirms the account belongs to the customer. The OpenBanking context's fetch_transactions retrieves transactions within the date range. The AuditRecording context's record logs the query. If the scope is missing or the account does not belong to the customer, the query is refused.

## Scenarios
### customer views transactions for own account within date range
Given: authenticated customer C-9281 holds scope transactions-read and owns account FI42 1234 5600 0007 85, which has three transactions between 2026-04-01 and 2026-04-15
When: show is invoked with account FI42 1234 5600 0007 85 and date range from 2026-04-01 to 2026-04-15
Then: three transactions are returned, and an audit entry records "showed transactions: 3 entries for account ending 0785" for customer C-9281

### customer attempts to view transactions for account they do not own
Given: authenticated customer C-9281 holds scope transactions-read, but account FI42 9999 0000 0001 00 belongs to customer C-6633
When: show is invoked with account FI42 9999 0000 0001 00
Then: the query is refused with reason "account does not belong to customer", no bank call is made, and an audit entry records "transaction query denied: ownership mismatch" for customer C-9281

### customer without transactions-read scope is refused
Given: authenticated customer C-4455 holds only scope accounts-read
When: show is invoked for account FI42 1234 5600 0007 85
Then: the query is refused with reason "missing required scope: transactions-read" and an audit entry records the denial
