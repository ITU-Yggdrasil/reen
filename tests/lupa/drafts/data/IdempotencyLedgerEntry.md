# Data: IdempotencyLedgerEntry

## Purpose
Records the outcome of a previously processed payment request, keyed by its idempotency token within a specific customer's scope. The ledger entry allows the system to return the original outcome for replayed requests without re-executing the transfer.

## Attributes
- token: the IdempotencyToken that was used in the original request
- outcome: the result of the original processing — one of "succeeded" or "failed", along with a text description
- timestamp: the moment the original request was processed, a date-and-time value with timezone
- customer id: the identifier of the customer who submitted the original request, a text value

## Invariants
- Token must be a valid, non-empty IdempotencyToken
- Outcome must be one of the recognised result kinds
- Timestamp must not be in the future
- Customer id must be non-empty
- The combination of token and customer id must be unique in the ledger (a token is scoped per customer)

## Scenarios
### Record a successful outcome
Given: token "tok-abc-001", outcome "succeeded" with description "Transfer of 25000 DKK to ACC-002 settled", timestamp "2026-03-15 10:31:00 CET", customer id "CUST-001"
When: an IdempotencyLedgerEntry is created
Then: the entry is accepted with all attributes set

### Record a failed outcome
Given: token "tok-abc-002", outcome "failed" with description "Insufficient funds", timestamp "2026-03-15 10:35:00 CET", customer id "CUST-001"
When: an IdempotencyLedgerEntry is created
Then: the entry is accepted with outcome "failed"

### Duplicate token for same customer is rejected
Given: an existing entry with token "tok-abc-001" and customer id "CUST-001"
When: another IdempotencyLedgerEntry is created with token "tok-abc-001" and customer id "CUST-001"
Then: the creation is rejected because the token already exists for this customer

### Same token for different customer is accepted
Given: an existing entry with token "tok-abc-001" and customer id "CUST-001"
When: an IdempotencyLedgerEntry is created with token "tok-abc-001" and customer id "CUST-002"
Then: the entry is accepted because the token is scoped per customer
