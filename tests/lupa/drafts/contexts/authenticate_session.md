# Context: AuthenticateSession

## Purpose
Resolves and verifies the session cookie on every incoming request, binding the request to an authenticated customer identity or producing a typed failure. This context guards every protected endpoint.

## Role Players
- request: IncomingRequest — carries a session cookie identifying the caller
- session_store: SessionStore — retrieves sessions by identifier
- clock: Clock — provides the current time for expiry and freshness checks

## Role Methods
### request
- resolve_session_cookie: extracts the session identifier from the request cookie header

### session_store
- lookup: retrieves the session record matching the extracted session identifier, or reports that no such session exists

### clock
- now: provides the current timestamp for expiry comparison

## Functionalities
### authenticate
The request's resolve_session_cookie extracts the session identifier. The session_store's lookup retrieves the session. The clock's now provides the current time, and the session is checked for expiry. If the session is live and its scopes satisfy the request, an AuthenticatedCustomer is returned carrying the customer identity, granted scopes, and correlation identifier. If the cookie is missing, the session is not found, or the session has expired, a typed failure is returned describing the reason.

## Scenarios
### valid session resolves to authenticated customer
Given: incoming request carries cookie lupa-session=S-00712, and session S-00712 exists with customer C-9281, scopes accounts-read and transactions-read, expiring at 2026-04-22T15:00:00Z
When: authenticate is invoked and the clock reads 2026-04-22T14:30:00Z
Then: an AuthenticatedCustomer is returned with customer identity C-9281, scopes accounts-read and transactions-read, and a fresh correlation identifier

### expired session returns typed failure
Given: incoming request carries cookie lupa-session=S-00710, and session S-00710 exists with customer C-9281, expired at 2026-04-22T13:00:00Z
When: authenticate is invoked and the clock reads 2026-04-22T14:30:00Z
Then: a typed failure "session expired" is returned and no AuthenticatedCustomer is produced

### missing cookie returns typed failure
Given: incoming request carries no session cookie
When: authenticate is invoked
Then: a typed failure "no session cookie" is returned and no AuthenticatedCustomer is produced
