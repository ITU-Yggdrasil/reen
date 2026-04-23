# Context: TransferInternational

## Purpose
Initiates a SWIFT international payment from the authenticated customer's account to a recipient identified by IBAN and BIC. The operation is idempotency-guarded and requires the payments-initiate-international scope.

## Role Players
- authenticated_customer: AuthenticatedCustomer — the verified customer identity
- source: Account — the account to debit
- recipient: IBAN — the payee's international bank account number
- recipient_bic: BIC — the payee's bank identifier code
- recipient_name: the human-readable name of the payee
- amount: Money — the sum to transfer
- message: a free-text payment reference or message
- currency: the target currency for the transfer (may differ from source account currency)
- idempotency_token: IdempotencyToken — the client-supplied unique token preventing duplicate execution
- idempotency_ledger: IdempotencyLedger — stores token-to-outcome mappings
- bank: @nordea-openbanking — the payment initiation endpoint
- audit_sink: AuditSink — receives an audit record of the transfer

## Props
- required_scope: payments-initiate-international

## Role Methods
### authenticated_customer
- verify_scope: confirms the session grants payments-initiate-international
- verify_source_ownership: confirms the source account belongs to this customer

### idempotency_ledger
- enforce: checks whether the idempotency token has been seen; if so, returns the stored outcome

### bank
- initiate_international_payment: submits the SWIFT payment order to Nordea with source, recipient IBAN, recipient BIC, recipient name, amount, currency, and message, returning a payment reference

### audit_sink
- record: appends an audit entry describing the international transfer and its outcome

## Functionalities
### transfer
The authenticated_customer's verify_scope confirms payments-initiate-international. The authenticated_customer's verify_source_ownership confirms the source account belongs to the customer. The idempotency_ledger's enforce checks the token. If already seen, the stored outcome is returned. Otherwise, the bank's initiate_international_payment submits the SWIFT payment. The outcome is recorded in the idempotency ledger and the audit_sink's record logs the transfer.

## Scenarios
### successful international SWIFT transfer
Given: authenticated customer C-9281 holds scope payments-initiate-international, owns source account FI42 1234 5600 0007 85 (balance 12450.00 EUR), recipient IBAN is DE89 3704 0044 0532 0130 00, recipient BIC is COBADEFFXXX, recipient name is "Schmidt GmbH", amount is 3500.00 EUR, currency is EUR, message is "Contract DE-2026-118", idempotency token idem-INT-20260422-001 is new
When: transfer is invoked
Then: the bank accepts the payment and returns reference PAY-30567, the idempotency ledger records the outcome, and an audit entry records "international transfer: 3500.00 EUR to DE89370400440532013000 via COBADEFFXXX, reference PAY-30567" for customer C-9281

### cross-currency international transfer
Given: authenticated customer C-9281 holds scope payments-initiate-international, owns source account FI42 1234 5600 0007 85 (denominated in EUR), recipient IBAN is GB29 NWBK 6016 1331 9268 19, recipient BIC is NWBKGB2L, recipient name is "Acme Ltd", amount is 1200.00 GBP, currency is GBP, message is "PO-44821", idempotency token idem-INT-20260422-002 is new
When: transfer is invoked
Then: the bank accepts the payment with currency conversion and returns reference PAY-30568, and an audit entry records "international transfer: 1200.00 GBP to GB29NWBK60161331926819, reference PAY-30568" for customer C-9281

### missing scope is refused
Given: authenticated customer C-4455 holds only scope payments-initiate-domestic and does not hold payments-initiate-international
When: transfer is invoked for 500.00 USD to recipient IBAN US12 3456 7890 1234 5678 90
Then: the transfer is refused with reason "missing required scope: payments-initiate-international", no bank call is made, and an audit entry records the denial
