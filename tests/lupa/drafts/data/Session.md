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
- scopes: the consent scope identifiers granted during authentication, a list of text values

## Invariants
- Customer id must be non-empty
- Bearer token must be non-empty
- Expiry must be after the created-at timestamp
- Refresh token must be non-empty
- CSRF token must be non-empty
- Chat state must be one of the recognised states "idle", "awaiting-input", "processing", or "confirming"
- A chat-state change is permitted only if it appears in the chat-state transition table below; every other transition is invalid
- Last activity must not be before created-at

## Chat State Transitions
The chat state moves through a finite state machine. The complete set of valid transitions is:

- "idle" → "awaiting-input" (the session begins accepting user input)
- "awaiting-input" → "processing" (the user submits a message)
- "awaiting-input" → "idle" (the user closes the prompt without sending)
- "processing" → "awaiting-input" (the assistant produces a plain reply that needs no confirmation)
- "processing" → "confirming" (the assistant produces a confirmation card requiring approval)
- "confirming" → "processing" (the user approves the card and the operation continues)
- "confirming" → "awaiting-input" (the user rejects the card and returns to free input)

Any transition not listed above is rejected. In particular, "idle" → "confirming", "idle" → "processing", and "confirming" → "idle" are not valid transitions.

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

### Awaiting-input returns to idle on cancel
Given: a Session with chat state "awaiting-input"
When: transition chat state to "idle" is performed
Then: the chat state becomes "idle"

### Processing produces a confirmation card
Given: a Session with chat state "processing"
When: transition chat state to "confirming" is performed
Then: the chat state becomes "confirming"

### Processing produces a plain reply
Given: a Session with chat state "processing"
When: transition chat state to "awaiting-input" is performed
Then: the chat state becomes "awaiting-input"

### Confirming proceeds on approval
Given: a Session with chat state "confirming"
When: transition chat state to "processing" is performed
Then: the chat state becomes "processing"

### Confirming returns to awaiting-input on rejection
Given: a Session with chat state "confirming"
When: transition chat state to "awaiting-input" is performed
Then: the chat state becomes "awaiting-input"

### Confirming cannot return directly to idle
Given: a Session with chat state "confirming"
When: transition chat state to "idle" is performed
Then: the transition is rejected because "confirming" to "idle" is not a valid state change
