# Data: TransferReceipt

## Purpose
Represents the confirmation returned by Nordea after a transfer has been submitted. It carries the external payment identifier assigned by the bank, the processing status, and the timestamp of acceptance, serving as proof that the payment instruction was received.

## Attributes
- external payment id: the unique payment identifier assigned by Nordea, a text value (e.g. "PAY-NDA-20260315-0001")
- status: the bank-reported processing state — one of "accepted", "processing", "settled", or "failed"
- timestamp: the moment Nordea acknowledged the payment instruction, a date-and-time value with timezone

## Invariants
- External payment id must be non-empty
- Status must be one of the recognised processing states
- Timestamp must not be in the future relative to the system clock at creation time

## Scenarios
### Accepted transfer receipt
Given: external payment id "PAY-NDA-20260315-0001", status "accepted", timestamp "2026-03-15 10:30:00 CET"
When: a TransferReceipt is created
Then: the TransferReceipt is accepted with all attributes set

### Settled transfer receipt
Given: external payment id "PAY-NDA-20260315-0002", status "settled", timestamp "2026-03-15 14:00:00 CET"
When: a TransferReceipt is created
Then: the TransferReceipt is accepted with status "settled"

### Empty external payment id is rejected
Given: external payment id "", status "accepted", timestamp "2026-03-15 10:30:00 CET"
When: a TransferReceipt is created
Then: the creation is rejected because the external payment id is empty

### Invalid status is rejected
Given: external payment id "PAY-NDA-20260315-0003", status "completed", timestamp "2026-03-15 10:30:00 CET"
When: a TransferReceipt is created
Then: the creation is rejected because "completed" is not a recognised status
