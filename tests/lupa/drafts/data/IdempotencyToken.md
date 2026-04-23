# Data: IdempotencyToken

## Purpose
An opaque token attached to payment requests to guarantee at-most-once processing. The ledger enforces uniqueness so that retried or duplicated requests do not result in double transfers.

## Attributes
- value: the token itself, an opaque text value (e.g. "tok-abc-001")

## Invariants
- The value must be non-empty
- The value must be unique per customer as enforced by the idempotency ledger
- The token is opaque: no structure is assumed beyond being a non-empty text value

## Scenarios
### Valid token
Given: the text "tok-abc-001"
When: an IdempotencyToken is created
Then: the IdempotencyToken is accepted with value "tok-abc-001"

### Empty token is rejected
Given: the text ""
When: an IdempotencyToken is created
Then: the creation is rejected because the value is empty

### Whitespace-only token is rejected
Given: the text "   "
When: an IdempotencyToken is created
Then: the creation is rejected because the value is effectively empty after trimming
