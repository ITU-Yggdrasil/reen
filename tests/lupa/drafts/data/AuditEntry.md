# Data: AuditEntry

## Purpose
Represents a single record in the append-only audit log, capturing who did what, when, and with what result. Audit entries provide the compliance trail for all significant actions in the system, with personal data redacted from the descriptor to ensure log safety.

## Attributes
- timestamp: the moment the event occurred, a date-and-time value with timezone
- customer id: the identifier of the customer who triggered the event, a text value
- event kind: the classification of what happened — e.g. "transfer-initiated", "transfer-settled", "login", "consent-granted", "chat-message-sent"
- correlation id: a text value linking related audit entries across a single business operation (e.g. all entries for one transfer share the same correlation id)
- outcome: the result of the event — one of "success", "failure", or "denied"
- redacted descriptor: a human-readable summary of the event with all personal data (names, account numbers, IBANs) replaced by masked tokens, a text value

## Invariants
- All attributes must be present; none may be absent
- Timestamp must not be in the future
- Customer id must be non-empty
- Event kind must be non-empty
- Correlation id must be non-empty
- Outcome must be one of the recognised result kinds
- The redacted descriptor must not contain raw account numbers, IBANs, or customer names — only masked tokens
- Audit entries are append-only: once created, they cannot be modified or deleted

## Scenarios
### Successful transfer initiation
Given: timestamp "2026-03-15 10:30:05 CET", customer id "CUST-001", event kind "transfer-initiated", correlation id "corr-7890", outcome "success", redacted descriptor "Transfer of 250.00 DKK from account ****6243 to account ****3210"
When: an AuditEntry is created
Then: the AuditEntry is accepted with all attributes set

### Failed transfer due to insufficient funds
Given: timestamp "2026-03-15 10:35:00 CET", customer id "CUST-001", event kind "transfer-initiated", correlation id "corr-7891", outcome "failure", redacted descriptor "Transfer of 50000.00 DKK from account ****6243 rejected: insufficient funds"
When: an AuditEntry is created
Then: the AuditEntry is accepted with outcome "failure"

### Descriptor containing raw IBAN is rejected
Given: timestamp "2026-03-15 10:30:05 CET", customer id "CUST-001", event kind "transfer-initiated", correlation id "corr-7892", outcome "success", redacted descriptor "Transfer to DK5000400440116243"
When: an AuditEntry is created
Then: the creation is rejected because the descriptor contains an unmasked IBAN

### Empty correlation id is rejected
Given: timestamp "2026-03-15 10:30:05 CET", customer id "CUST-001", event kind "transfer-initiated", correlation id "", outcome "success", redacted descriptor "Transfer of 250.00 DKK"
When: an AuditEntry is created
Then: the creation is rejected because the correlation id is empty
