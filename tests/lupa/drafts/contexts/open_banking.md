# Context: OpenBanking

## Purpose
Provides the integration layer with Nordea's PSD2 Open Banking API. All bank interactions — account retrieval, transaction queries, payment initiation, and token management — flow through this context. Other contexts reference OpenBanking rather than interacting with the bank directly.

## Role Players
- session: Session — the session holding the bearer token used to authenticate API calls
- account: Account — the account targeted by a banking operation

## Props
- api_base_url: the base URL of the Nordea Open Banking API

## Role Methods
### session
- provide_bearer_token: supplies the current bearer token for authenticating API requests
- provide_refresh_token: supplies the refresh token for token renewal

### account
- provide_identifier: supplies the account identifier for account-specific API calls

## Functionalities
### fetch_accounts
Uses the session's bearer token to retrieve the list of accounts belonging to the customer from the bank's accounts endpoint.

### fetch_transactions
Uses the session's bearer token and the account's identifier to retrieve transactions for a specific account within a date range from the bank's transactions endpoint.

### initiate_own_transfer
Uses the session's bearer token to submit a payment order transferring funds between two accounts owned by the same customer. Returns a payment reference on success.

### initiate_domestic_payment
Uses the session's bearer token to submit a domestic payment order with source account, recipient domestic account number, amount, recipient name, and message. Returns a payment reference on success.

### initiate_international_payment
Uses the session's bearer token to submit a SWIFT international payment order with source account, recipient IBAN, recipient BIC, recipient name, amount, target currency, and message. Returns a payment reference on success.

### exchange_code_for_token
Exchanges an authorization code received from the consent redirect for a bearer token, refresh token, and expiry duration.

### exchange_refresh_token
Exchanges a refresh token for a new bearer token, refresh token, and expiry duration. Makes exactly one attempt with no retry.

### revoke_token
Sends a revocation request to the bank for a bearer token, invalidating it on the bank side.

## Scenarios
### fetch accounts for authenticated session
Given: session S-00712 holds a valid bearer token
When: fetch_accounts is invoked
Then: the bank returns the list of accounts associated with the session's customer

### fetch transactions within date range
Given: session S-00712 holds a valid bearer token, account FI42 1234 5600 0007 85
When: fetch_transactions is invoked for account FI42 1234 5600 0007 85 with date range 2026-04-01 to 2026-04-15
Then: the bank returns the matching transactions for that account and period

### initiate own-account transfer
Given: session S-00712 holds a valid bearer token, source account FI42 1234 5600 0007 85, destination account FI42 1234 5600 0007 86, amount 2000.00 EUR
When: initiate_own_transfer is invoked
Then: the bank accepts the payment and returns reference PAY-10221

### exchange authorization code for tokens
Given: authorization code NRD-8827-AXKP was received from the consent redirect
When: exchange_code_for_token is invoked with code NRD-8827-AXKP
Then: the bank returns bearer token eyJ0b2tlbi, refresh token rf-ABX9201, and expiry 3600 seconds

### revoke bearer token
Given: session S-00712 holds bearer token eyJ0b2tlbi
When: revoke_token is invoked
Then: the bank invalidates the bearer token
