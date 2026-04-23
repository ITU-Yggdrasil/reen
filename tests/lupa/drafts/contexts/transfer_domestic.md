# Context: TransferDomestic

## Purpose
Initiates a domestic payment from the authenticated customer's account to a recipient identified by a domestic account number. The operation is idempotency-guarded and requires the payments-initiate-domestic scope.

## Role Players
- authenticated_customer: AuthenticatedCustomer — the verified customer identity
- source: Account — the account to debit
- recipient: DomesticAccountNumber — the payee's domestic account number
- recipient_name: RecipientName — the human-readable name of the payee
- amount: Money — the sum to transfer
- message: PaymentMessage — a free-text payment message visible to both parties
- idempotency_token: IdempotencyToken — the client-supplied unique token preventing duplicate execution
- idempotency_ledger: IdempotencyLedger — stores token-to-outcome mappings
- bank: NordeaClient — the payment initiation endpoint
- audit_sink: AuditSink — receives an audit record of the transfer

## Props
- required_scope: payments-initiate-domestic

## Role Methods
### authenticated_customer
- verify_scope: confirms the session grants payments-initiate-domestic
- verify_source_ownership: confirms the source account belongs to this customer

### idempotency_ledger
- enforce: checks whether the idempotency token has been seen; if so, returns the stored outcome

### bank
- initiate_domestic_payment: submits the domestic payment order to Nordea with source, recipient, amount, recipient name, and message, returning a payment reference

### audit_sink
- record: appends an audit entry describing the domestic transfer and its outcome

## Functionalities
### transfer
The authenticated_customer's verify_scope confirms payments-initiate-domestic. The authenticated_customer's verify_source_ownership confirms the source account belongs to the customer. The idempotency_ledger's enforce checks the token. If already seen, the stored outcome is returned. Otherwise, the bank's initiate_domestic_payment submits the payment order. The outcome is recorded in the idempotency ledger and the audit_sink's record logs the transfer.

## Scenarios
### successful domestic transfer
Given: authenticated customer C-9281 holds scope payments-initiate-domestic, owns source account FI42 1234 5600 0007 85 (balance 12450.00 EUR), recipient account number is 1234-56789012, recipient name is "Elisa Oyj", amount is 89.90 EUR, message is "Invoice 2026-04-001", idempotency token idem-DOM-20260422-001 is new
When: transfer is invoked
Then: the bank accepts the payment and returns reference PAY-20334, the idempotency ledger records the outcome, and an audit entry records "domestic transfer: 89.90 EUR to 1234-56789012, reference PAY-20334" for customer C-9281

### source account does not belong to customer
Given: authenticated customer C-4455 holds scope payments-initiate-domestic, but source account FI42 1234 5600 0007 85 belongs to customer C-9281
When: transfer is invoked for 50.00 EUR to recipient 9876-00001234
Then: the transfer is refused with reason "source account does not belong to customer", no bank call is made, and an audit entry records the denial for customer C-4455

### bank rejects the payment
Given: authenticated customer C-9281 holds scope payments-initiate-domestic, owns source account FI42 1234 5600 0007 85, recipient is 1234-56789012, amount is 99999.00 EUR, idempotency token idem-DOM-20260422-002 is new
When: transfer is invoked and the bank rejects the payment with reason "insufficient funds"
Then: the failure is recorded in the idempotency ledger, and an audit entry records "domestic transfer failed: insufficient funds" for customer C-9281
