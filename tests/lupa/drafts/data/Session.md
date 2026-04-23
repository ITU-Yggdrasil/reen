# Data: Session

## Purpose
Represents an authenticated banking session for a customer, carrying the OAuth tokens, security tokens, and timing information needed to maintain a secure connection with the Nordea API and the chat interface.

## Attributes
- customer id: the identifier of the authenticated customer, a text value
- bearer token: the OAuth access token used for API calls, a text value
- expiry: the moment the bearer token expires, a date-and-time value with timezone
- refresh token: the token used to obtain a new bearer token before expiry, a text value
- csrf token: the cross-site request forgery prevention token for the web interface, a text value
- chat state: the current state of the chat interface — one of "idle", "awaiting-input", "processing", or "confirming"
- created at: the moment the session was established, a date-and-time value with timezone
- last activity at: the moment of the most recent interaction, a date-and-time value with timezone

## Invariants
- Customer id must be non-empty
- Bearer token must be non-empty
- Expiry must be after the created-at timestamp
- Refresh token must be non-empty
- CSRF token must be non-empty
- Chat state must be one of the recognised states
- Last activity must not be before created-at

## Operations
### is expired
Determines whether the bearer token has passed its expiry time relative to the current moment. Returns an affirmative or negative answer.

### touch
Updates the last-activity-at timestamp to the current moment, indicating ongoing user interaction.

### transition chat state
Moves the chat state to a new recognised state. Rejects transitions that are not valid (e.g. from "idle" directly to "confirming" without going through "processing").

## Scenarios
### Create a fresh session
Given: customer id "CUST-001", bearer token "eyJ..abc", expiry "2026-03-15 11:30:00 CET", refresh token "ref-xyz", csrf token "csrf-12345", chat state "idle", created at "2026-03-15 10:30:00 CET", last activity at "2026-03-15 10:30:00 CET"
When: a Session is created
Then: the Session is accepted with all attributes set

### Session is not expired before expiry time
Given: a Session with expiry "2026-03-15 11:30:00 CET" and current time is "2026-03-15 11:00:00 CET"
When: is expired is checked
Then: the answer is negative

### Session is expired after expiry time
Given: a Session with expiry "2026-03-15 11:30:00 CET" and current time is "2026-03-15 12:00:00 CET"
When: is expired is checked
Then: the answer is affirmative

### Touch updates last activity
Given: a Session with last activity at "2026-03-15 10:30:00 CET" and current time is "2026-03-15 10:45:00 CET"
When: touch is performed
Then: the last activity at becomes "2026-03-15 10:45:00 CET"

### Valid chat state transition
Given: a Session with chat state "idle"
When: transition chat state to "awaiting-input" is performed
Then: the chat state becomes "awaiting-input"

### Invalid chat state transition is rejected
Given: a Session with chat state "idle"
When: transition chat state to "confirming" is performed
Then: the transition is rejected because "idle" to "confirming" is not a valid state change
