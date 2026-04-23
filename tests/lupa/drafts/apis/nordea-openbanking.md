# Nordea OpenBanking Personal API

## Purpose

Lupa reads account and transaction data and initiates payments via Nordea's OpenBanking Personal interface. This is the sole external banking surface — Lupa does not hand-author banking primitives, it consumes them from Nordea's API.

## Source

- OpenAPI Local: nordea-openbanking.yaml
- Documentation URL: https://developer.nordeaopenbanking.com/

## Operations

- GetAccounts: list the authenticated customer's accounts
- GetAccountById: retrieve a single account by its identifier
- GetTransactions: list transactions for a specific account, with date-range filtering and pagination
- InitiateDomesticPayment: transfer to a domestic recipient by account number
- InitiateSEPAPayment: transfer within the SEPA zone using IBAN (also used for own-account transfers)
- InitiateInternationalPayment: transfer internationally via IBAN and BIC/SWIFT

## Notes

- Authentication: OAuth 2.0 with bearer token per request. The token is obtained through the Nordea consent flow with PSD2 scopes and Strong Customer Authentication.
- Required headers on every request: X-IBM-Client-Id, X-IBM-Client-Secret (TPP credentials), psu-ip-address (PSD2 requirement for the payment service user's IP), and Authorization (bearer token).
- Pagination: Nordea paginates list responses using a continuation key. The consuming context must coalesce pages up to a configurable maximum.
- The snapshot is trimmed to only the operations Lupa whitelists. Operations outside the whitelist (card management, standing orders, account opening) are excluded. Re-ingest from the full spec if the whitelist changes.
- PSD2 headers (psu-user-agent, tpp-redirect-uri) are required on payment initiation calls. Encode them in the operation specs; do not leave them as runtime-only concerns.
