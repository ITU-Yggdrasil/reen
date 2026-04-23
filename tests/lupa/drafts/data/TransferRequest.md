# Data: TransferRequest

## Purpose
Represents a specification for a money transfer, capturing all the details needed to instruct the bank to move funds. The request comes in per-kind variants depending on the destination: own-account transfers within the same customer, domestic transfers to another Danish account, and international transfers via SWIFT.

## Attributes
- kind: the transfer classification — one of "own", "domestic", or "international"
- source account: a reference to the Account funds are drawn from
- amount: the Money value to transfer
- description: a narrative for the transaction, a text value
- idempotency token: an IdempotencyToken ensuring the request is processed at most once

### Own-transfer attributes
- destination account: a reference to another Account held by the same customer

### Domestic-transfer attributes
- destination account number: the recipient's DomesticAccountNumber
- recipient name: the name of the recipient, a text value

### International-transfer attributes
- destination iban: the recipient's IBAN
- destination bic: the recipient's BIC
- recipient name: the name of the recipient, a text value
- charge bearer: who pays the transfer fees — one of "shared", "sender", or "recipient"

## Invariants
- Kind must be one of the recognised transfer classifications
- Source account must be a valid account reference
- Amount must be positive (greater than zero in minor units)
- Idempotency token must be present
- For own transfers, the destination account must belong to the same customer as the source account
- For own transfers, the source and destination must be different accounts
- For domestic transfers, the recipient name must be non-empty
- For international transfers, the destination IBAN, destination BIC, and recipient name must all be present
- For international transfers, charge bearer must be one of the recognised options

## Scenarios
### Own-account transfer
Given: kind "own", source account "ACC-001", destination account "ACC-002", amount of 25000 in DKK, description "Savings top-up", idempotency token "tok-abc-001"
When: a TransferRequest is created
Then: the TransferRequest is accepted as an own-account transfer

### Domestic transfer
Given: kind "domestic", source account "ACC-001", destination account number "0040-9876543210", recipient name "Lars Petersen", amount of 15000 in DKK, description "Rent April", idempotency token "tok-abc-002"
When: a TransferRequest is created
Then: the TransferRequest is accepted as a domestic transfer

### International transfer
Given: kind "international", source account "ACC-001", destination IBAN "DE89370400440532013000", destination BIC "COBADEFFXXX", recipient name "Hans Mueller", amount of 100000 in DKK, description "Invoice 4455", charge bearer "shared", idempotency token "tok-abc-003"
When: a TransferRequest is created
Then: the TransferRequest is accepted as an international transfer

### Zero amount is rejected
Given: kind "own", source account "ACC-001", destination account "ACC-002", amount of 0 in DKK, description "Nothing", idempotency token "tok-abc-004"
When: a TransferRequest is created
Then: the creation is rejected because the amount must be positive

### Own transfer to the same account is rejected
Given: kind "own", source account "ACC-001", destination account "ACC-001", amount of 5000 in DKK, description "Self", idempotency token "tok-abc-005"
When: a TransferRequest is created
Then: the creation is rejected because source and destination must differ

### International transfer without BIC is rejected
Given: kind "international", source account "ACC-001", destination IBAN "DE89370400440532013000", destination BIC absent, recipient name "Hans Mueller", amount of 100000 in DKK, description "Invoice 4455", charge bearer "shared", idempotency token "tok-abc-006"
When: a TransferRequest is created
Then: the creation is rejected because the destination BIC must be present for international transfers
