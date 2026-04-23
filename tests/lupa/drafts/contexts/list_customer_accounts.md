# Context: ListCustomerAccounts

## Purpose
Retrieves the list of bank accounts belonging to the authenticated customer from the Open Banking integration. The customer must hold the accounts-read scope.

## Role Players
- authenticated_customer: AuthenticatedCustomer — the verified customer identity with granted scopes

## Props
- required_scope: accounts-read

## Role Methods
### authenticated_customer
- verify_scope: confirms the customer's session grants the accounts-read scope before any external call is made

## Functionalities
### list
The authenticated_customer's verify_scope confirms the accounts-read scope is present. The OpenBanking context's fetch_accounts retrieves the accounts from the bank. The AuditRecording context's record logs the action. If the scope is missing, the listing is refused and the audit entry reflects the denial.

## Scenarios
### customer with accounts-read scope lists two accounts
Given: authenticated customer C-9281 holds scope accounts-read, and the bank reports two accounts: FI42 1234 5600 0007 85 (current, balance 12450.00 EUR) and FI42 1234 5600 0007 86 (savings, balance 3200.00 EUR)
When: list is invoked
Then: both accounts are returned, and an audit entry records "listed accounts: 2 accounts" for customer C-9281

### customer without accounts-read scope is refused
Given: authenticated customer C-4455 holds only scope payments-initiate-own and does not hold accounts-read
When: list is invoked
Then: the listing is refused with reason "missing required scope: accounts-read", no bank call is made, and an audit entry records "list accounts denied: missing scope" for customer C-4455
