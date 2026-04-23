# Context: AuthenticateSession

## Purpose
Resolves and verifies the session cookie on every incoming request, binding the request to an authenticated customer identity or producing a typed failure. This context guards every protected endpoint.

## Role Players
- session: Session — the session record to validate against the extracted identifier

## Props
- session_identifier: the session identifier extracted from the incoming request's cookie header

## Role Methods
### session
- check_expiry: determines whether the session has expired by comparing its expiry timestamp to the current time
- check_scopes: confirms the session's granted scopes satisfy the requirements of the requested operation

## Functionalities
### authenticate
The session identifier prop is used to look up a session. The session's check_expiry determines whether the session is still live. The session's check_scopes verifies the granted permissions are sufficient. If the session is live and its scopes satisfy the request, an AuthenticatedCustomer is returned carrying the customer identity, granted scopes, and correlation identifier. If the identifier is missing, the session is not found, or the session has expired, a typed failure is returned describing the reason.

## Scenarios
### valid session resolves to authenticated customer
Given: session identifier is S-00712, and session S-00712 exists with customer C-9281, scopes accounts-read and transactions-read, expiring at 2026-04-22T15:00:00Z
When: authenticate is invoked and the current time is 2026-04-22T14:30:00Z
Then: an AuthenticatedCustomer is returned with customer identity C-9281, scopes accounts-read and transactions-read, and a fresh correlation identifier

### expired session returns typed failure
Given: session identifier is S-00710, and session S-00710 exists with customer C-9281, expired at 2026-04-22T13:00:00Z
When: authenticate is invoked and the current time is 2026-04-22T14:30:00Z
Then: a typed failure "session expired" is returned and no AuthenticatedCustomer is produced

### missing cookie returns typed failure
Given: no session identifier is provided
When: authenticate is invoked
Then: a typed failure "no session cookie" is returned and no AuthenticatedCustomer is produced
