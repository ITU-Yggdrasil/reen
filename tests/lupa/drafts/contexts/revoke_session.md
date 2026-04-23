# Context: RevokeSession

## Purpose
Performs a complete logout by revoking the bearer token at the bank, destroying the local session, and recording the event. After revocation, the session cannot be used or refreshed.

## Role Players
- authenticated_customer: AuthenticatedCustomer — the verified customer identity requesting logout
- session: Session — the session to be revoked, containing the bearer and refresh tokens

## Role Methods
### session
- provide_tokens: supplies the bearer token and refresh token for revocation at the bank
- destroy: marks the session as destroyed so it can no longer be resolved by future requests

## Functionalities
### revoke
The session's provide_tokens supplies the tokens. The OpenBanking context's revoke_token invalidates the bearer token at the bank. The session's destroy removes the session from use. The AuditRecording context's record logs the revocation. Even if the bank's revocation call fails (network error, timeout), the session is still destroyed — the bank token will expire naturally, and the customer cannot use the local session again.

## Scenarios
### successful logout with bank revocation
Given: authenticated customer C-9281 with session S-00712 holding bearer token eyJ0b2tlbi and refresh token rf-ABX9201
When: revoke is invoked
Then: the bearer token is revoked at the bank, session S-00712 is destroyed, and an audit entry records "session revoked" for customer C-9281 with correlation identifier corr-0061

### bank revocation fails but local session is still destroyed
Given: authenticated customer C-9281 with session S-00714 holding bearer token eyJleHBpcm, and the bank's revocation endpoint is unreachable
When: revoke is invoked
Then: the bank revocation attempt fails, but session S-00714 is still destroyed, and an audit entry records "session revoked (bank revocation failed: network timeout)" for customer C-9281 with correlation identifier corr-0062
