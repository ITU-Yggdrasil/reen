# Context: RefreshNordeaSession

## Purpose
Refreshes an expiring bearer token using the stored refresh token, making exactly one attempt. If the refresh fails for any reason, the session is expired immediately — the customer must re-authenticate from scratch.

## Role Players
- session: Session — the existing session whose bearer token is approaching expiry
- oauth_endpoint: NordeaClient — the token endpoint that exchanges a refresh token for a new bearer token
- session_store: SessionStore — persists the updated or expired session
- audit_sink: AuditSink — receives an immutable record of the refresh outcome

## Role Methods
### session
- provide_refresh_token: supplies the current refresh token for the exchange attempt
- accept_new_tokens: replaces the bearer and refresh tokens with the newly issued pair and updates the expiry

### oauth_endpoint
- exchange_refresh_token: sends the refresh token to the bank and returns a new bearer token, refresh token, and expiry — or an error

### session_store
- persist_updated: writes the session with its new tokens and expiry back to storage
- expire: marks the session as expired so it can no longer be used

### audit_sink
- record: appends an immutable entry describing the refresh outcome

## Functionalities
### refresh
The session's provide_refresh_token supplies the token. The oauth_endpoint's exchange_refresh_token makes exactly one attempt to obtain new credentials. On success, the session's accept_new_tokens updates the session, the session_store's persist_updated saves it, and the audit_sink's record logs the renewal. On failure, the session_store's expire marks the session dead and the audit_sink's record logs the failure. No retry is attempted.

## Scenarios
### successful token refresh
Given: session S-00712 has bearer token expiring in 45 seconds and refresh token rf-ABX9201
When: refresh is invoked for session S-00712
Then: the oauth_endpoint exchanges rf-ABX9201 for new bearer token eyJuZXdiZW, new refresh token rf-CDY1044, the session is updated with expiry 3600 seconds, and an audit entry records "session refreshed" for session S-00712

### refresh fails — session expired immediately
Given: session S-00713 has bearer token expiring in 30 seconds and refresh token rf-QWE5567 which the bank has revoked
When: refresh is invoked for session S-00713
Then: the oauth_endpoint rejects rf-QWE5567, the session is marked expired in the session store, no retry is attempted, and an audit entry records "session expired: refresh rejected" for session S-00713
