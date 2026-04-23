# Context: AuthenticateWithNordea

## Purpose
Enacts the OAuth consent flow against Nordea's PSD2 Open Banking API, guiding the customer through Strong Customer Authentication before establishing a local session. This is the only entry point for new sessions.

## Role Players
- user: IncomingRequest — the unauthenticated party initiating login
- oauth_endpoint: NordeaClient — the external authorization server that issues tokens after consent
- session_store: SessionStore — persists sessions for later retrieval
- audit_sink: AuditSink — receives immutable audit records of authentication events

## Props
- required_scopes: the PSD2 permission scopes requested during consent (accounts-read, transactions-read, payments-initiate-own, payments-initiate-domestic, payments-initiate-international)

## Role Methods
### user
- redirect_to_consent: constructs the consent URL with required scopes and redirects the user to the bank's authorization page

### oauth_endpoint
- exchange_code_for_token: exchanges the authorization code returned from the consent redirect for a bearer token and refresh token
- sca_challenge_complete: verifies that the Strong Customer Authentication challenge was fully completed before proceeding

### session_store
- persist: creates a new session record containing the bearer token, refresh token, granted scopes, and expiry

### audit_sink
- record: appends an immutable entry describing the authentication outcome, including timestamp, correlation identifier, and result

## Functionalities
### authenticate
The user's redirect_to_consent role method builds the authorization URL and sends the user to the bank. When the user returns with an authorization code, the oauth_endpoint's exchange_code_for_token obtains the bearer and refresh tokens. The oauth_endpoint's sca_challenge_complete then confirms that Strong Customer Authentication was satisfied. Only after SCA confirmation does the session_store's persist create a session. Finally the audit_sink's record logs the outcome. If SCA did not complete, no session is created and the audit entry reflects the failure.

## Scenarios
### successful authentication with full scopes
Given: a user arrives without a session, requesting scopes accounts-read and transactions-read
When: authenticate is invoked and the user completes consent at Nordea, returning authorization code NRD-8827-AXKP
Then: the oauth_endpoint exchanges NRD-8827-AXKP for bearer token eyJ0b2tlbi, SCA is confirmed, a session is persisted with expiry 3600 seconds, and an audit entry records "authentication succeeded" with correlation identifier corr-0041

### SCA not completed — session must not be created
Given: a user arrives without a session, requesting scope payments-initiate-own
When: authenticate is invoked and the user returns authorization code NRD-5513-BQLE but the SCA challenge was not completed at the bank
Then: no session is created, the oauth_endpoint reports SCA incomplete, and an audit entry records "authentication failed: SCA not completed" with correlation identifier corr-0042

### consent denied by user
Given: a user arrives without a session
When: authenticate is invoked but the user denies consent at the Nordea authorization page, returning an error callback
Then: no authorization code is exchanged, no session is created, and an audit entry records "authentication failed: consent denied" with correlation identifier corr-0043
