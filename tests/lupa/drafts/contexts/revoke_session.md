# Context: RevokeSession

## Purpose
Performs a complete logout by revoking the bearer token at Nordea, destroying the local session, and recording the event. After revocation, the session cannot be used or refreshed.

## Role Players
- authenticated_customer: AuthenticatedCustomer — the verified customer identity requesting logout
- session: Session — the session to be revoked, containing the bearer and refresh tokens
- session_store: SessionStore — removes the session from persistent storage
- oauth_endpoint: NordeaClient — the token revocation endpoint at the bank
- audit_sink: AuditSink — receives an audit record of the revocation

## Role Methods
### session
- provide_tokens: supplies the bearer token and refresh token for revocation at the bank

### oauth_endpoint
- revoke_token: sends a revocation request to Nordea for the bearer token, invalidating it at the bank side

### session_store
- destroy: removes the session from persistent storage so it can no longer be resolved by future requests

### audit_sink
- record: appends an audit entry describing the session revocation

## Functionalities
### revoke
The session's provide_tokens supplies the tokens. The oauth_endpoint's revoke_token invalidates the bearer token at the bank. The session_store's destroy removes the session from local storage. The audit_sink's record logs the revocation. Even if the bank's revocation call fails (network error, timeout), the local session is still destroyed — the bank token will expire naturally, and the customer cannot use the local session again.

## Scenarios
### successful logout with bank revocation
Given: authenticated customer C-9281 with session S-00712 holding bearer token eyJ0b2tlbi and refresh token rf-ABX9201
When: revoke is invoked
Then: the oauth_endpoint revokes bearer token eyJ0b2tlbi at Nordea, session S-00712 is destroyed in the session store, and an audit entry records "session revoked" for customer C-9281 with correlation identifier corr-0061

### bank revocation fails but local session is still destroyed
Given: authenticated customer C-9281 with session S-00714 holding bearer token eyJleHBpcm, and Nordea's revocation endpoint is unreachable
When: revoke is invoked
Then: the oauth_endpoint revocation attempt fails, but session S-00714 is still destroyed in the session store, and an audit entry records "session revoked (bank revocation failed: network timeout)" for customer C-9281 with correlation identifier corr-0062
