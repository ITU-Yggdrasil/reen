# Context: RefreshNordeaSession

## Purpose
Refreshes an expiring bearer token using the stored refresh token, making exactly one attempt. If the refresh fails for any reason, the session is expired immediately — the customer must re-authenticate from scratch.

## Role Players
- session: Session — the existing session whose bearer token is approaching expiry

## Role Methods
### session
- provide_refresh_token: supplies the current refresh token for the exchange attempt
- accept_new_tokens: replaces the bearer and refresh tokens with the newly issued pair and updates the expiry
- mark_expired: marks the session as expired so it can no longer be used

## Functionalities
### refresh
The session's provide_refresh_token supplies the token. The OpenBanking context's exchange_refresh_token makes exactly one attempt to obtain new credentials. On success, the session's accept_new_tokens updates the session and persists the changes. The AuditRecording context's record logs the renewal. On failure, the session's mark_expired renders the session unusable and the AuditRecording context's record logs the failure. No retry is attempted.

## Scenarios
### successful token refresh
Given: session S-00712 has bearer token expiring in 45 seconds and refresh token rf-ABX9201
When: refresh is invoked for session S-00712
Then: the refresh token is exchanged for new bearer token eyJuZXdiZW, new refresh token rf-CDY1044, the session is updated with expiry 3600 seconds, and an audit entry records "session refreshed" for session S-00712

### refresh fails — session expired immediately
Given: session S-00713 has bearer token expiring in 30 seconds and refresh token rf-QWE5567 which the bank has revoked
When: refresh is invoked for session S-00713
Then: the refresh token exchange is rejected, the session is marked expired, no retry is attempted, and an audit entry records "session expired: refresh rejected" for session S-00713
