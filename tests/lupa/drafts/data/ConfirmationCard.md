# Data: ConfirmationCard

## Purpose
Represents a short-lived, single-use confirmation prompt shown to the customer before a transfer is executed. The card displays the proposed transfer details, carries an idempotency token for at-most-once execution, and includes a signed token to prevent tampering. Once rendered, the customer must accept or reject it within its validity window.

## Attributes
- source account display: a text summary of the source account (e.g. "Loenmodtagerkonto ****6243")
- destination display: a text summary of the destination (e.g. "Savings ****3210" or "Lars Petersen, 0040-9876543210")
- amount display: the transfer amount rendered in major units with currency (e.g. "250.00 DKK")
- description: the transfer narrative, a text value
- transfer kind: the classification of the transfer — one of "own", "domestic", "international"
- idempotency token: the IdempotencyToken that will be used if the transfer is executed
- signed token: a cryptographically signed text value binding the card contents to prevent modification after rendering
- valid until: the moment after which the card can no longer be accepted, a date-and-time value with timezone
- used: whether this card has already been accepted or rejected — initially negative
- customer id: the identifier of the customer for whom this card was issued, a text value

## Invariants
- All display fields must be non-empty
- The signed token must be non-empty
- Valid-until must be in the future at the time of card creation
- The card is single-use: once accepted or rejected, it cannot be acted upon again
- The validity window should be short-lived (typically a few minutes)
- Transfer kind must be one of the recognised classifications

## Operations
### is valid
Determines whether the card can still be acted upon: it has not been used and the current moment is before the valid-until time.

### mark used
Records that the card has been accepted or rejected, preventing further use.

## Scenarios
### Create a valid confirmation card
Given: source account display "Loenmodtagerkonto ****6243", destination display "Savings ****3210", amount display "250.00 DKK", description "Savings top-up", transfer kind "own", idempotency token "tok-abc-001", signed token "sig-xyz-999", valid until "2026-03-15 10:35:00 CET", used is negative, and current time is "2026-03-15 10:31:00 CET"
When: a ConfirmationCard is created
Then: the ConfirmationCard is accepted with all attributes set

### Card is valid before expiry and unused
Given: a ConfirmationCard with valid until "2026-03-15 10:35:00 CET", used is negative, and current time is "2026-03-15 10:33:00 CET"
When: is valid is checked
Then: the answer is affirmative

### Card is invalid after expiry
Given: a ConfirmationCard with valid until "2026-03-15 10:35:00 CET", used is negative, and current time is "2026-03-15 10:36:00 CET"
When: is valid is checked
Then: the answer is negative

### Card is invalid after use
Given: a ConfirmationCard with valid until "2026-03-15 10:35:00 CET" and used is affirmative
When: is valid is checked
Then: the answer is negative

### Mark used on a fresh card
Given: a ConfirmationCard with used is negative
When: mark used is performed
Then: the used attribute becomes affirmative

### Mark used on an already-used card is rejected
Given: a ConfirmationCard with used is affirmative
When: mark used is performed
Then: the operation is rejected because the card has already been used
