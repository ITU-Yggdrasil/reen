# Context: AuthenticateWithNordea

## Purpose
Enacts the OAuth consent flow against Nordea's PSD2 Open Banking API, guiding the customer through Strong Customer Authentication before establishing a local session. This is the only entry point for new sessions.

## Role Players
- session: Session — the session to be created once authentication succeeds

## Props
- authorization_code: the authorization code returned from the bank's consent redirect
- required_scopes: the PSD2 permission scopes requested during consent (accounts-read, transactions-read, payments-initiate-own, payments-initiate-domestic, payments-initiate-international)

## Role Methods
### session
- populate: fills the session with the bearer token, refresh token, granted scopes, and expiry obtained from the token exchange

## Functionalities
### authenticate
The customer is redirected to the bank's authorization page with the required scopes. When the customer returns with an authorization code, the OpenBanking context's exchange_code_for_token obtains the bearer and refresh tokens. Strong Customer Authentication completion is verified before proceeding. Only after SCA confirmation does the session's populate create a session record. Finally the AuditRecording context's record logs the outcome. If SCA did not complete, no session is created and the audit entry reflects the failure.

## Scenarios
### successful authentication with full scopes
Given: a user arrives without a session, requesting scopes accounts-read and transactions-read
When: authenticate is invoked and the user completes consent at Nordea, returning authorization code NRD-8827-AXKP
Then: the authorization code is exchanged for bearer token eyJ0b2tlbi, SCA is confirmed, a session is populated with expiry 3600 seconds, and an audit entry records "authentication succeeded" with correlation identifier corr-0041

### SCA not completed — session must not be created
Given: a user arrives without a session, requesting scope payments-initiate-own
When: authenticate is invoked and the user returns authorization code NRD-5513-BQLE but the SCA challenge was not completed at the bank
Then: no session is created, SCA incomplete is reported, and an audit entry records "authentication failed: SCA not completed" with correlation identifier corr-0042

### consent denied by user
Given: a user arrives without a session
When: authenticate is invoked but the user denies consent at the Nordea authorization page, returning an error callback
Then: no authorization code is exchanged, no session is created, and an audit entry records "authentication failed: consent denied" with correlation identifier corr-0043
