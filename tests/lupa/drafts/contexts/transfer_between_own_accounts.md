# Context: TransferBetweenOwnAccounts

## Purpose
Initiates a transfer between two accounts that both belong to the same authenticated customer via the Open Banking integration. Both accounts must belong to the customer, and the operation is idempotency-guarded.

## Role Players
- authenticated_customer: AuthenticatedCustomer — the verified customer identity
- source: Account — the account to debit
- destination: Account — the account to credit
- amount: Money — the sum to transfer
- idempotency_token: IdempotencyToken — the client-supplied unique token preventing duplicate execution

## Props
- required_scope: payments-initiate-own

## Role Methods
### source
- verify_sufficient_funds: checks that the source account balance covers the transfer amount

### destination
- accept_credit: receives the transferred amount

### amount
(no methods — this role is used as a passive value)

### idempotency_token
(no methods — this role is used as a passive value)

### authenticated_customer
- verify_scope: confirms the session grants payments-initiate-own
- verify_ownership_of_both: confirms both the source and destination accounts belong to this customer

## Functionalities
### transfer
The authenticated_customer's verify_scope confirms payments-initiate-own. The authenticated_customer's verify_ownership_of_both confirms both accounts belong to the customer. The EnforceIdempotency context checks the token. If the token has been seen, the stored outcome is returned. Otherwise, the OpenBanking context's initiate_own_transfer submits the payment and the outcome is recorded. The AuditRecording context's record logs the transfer. If either account does not belong to the customer, the transfer is refused.

## Scenarios
### successful own-account transfer
Given: authenticated customer C-9281 holds scope payments-initiate-own, owns source account FI42 1234 5600 0007 85 (balance 12450.00 EUR) and destination account FI42 1234 5600 0007 86, idempotency token idem-OWN-20260422-001 is new
When: transfer is invoked for 2000.00 EUR from source to destination
Then: the bank accepts the payment and returns reference PAY-10221, the outcome is recorded against the idempotency token, and an audit entry records "own-account transfer: 2000.00 EUR, reference PAY-10221" for customer C-9281

### destination account belongs to a different customer
Given: authenticated customer C-9281 holds scope payments-initiate-own, owns source account FI42 1234 5600 0007 85, but destination account FI42 9999 0000 0001 00 belongs to customer C-6633
When: transfer is invoked for 500.00 EUR
Then: the transfer is refused with reason "destination account does not belong to customer", no bank call is made, and an audit entry records the denial

### duplicate idempotency token returns stored outcome
Given: authenticated customer C-9281 submits idempotency token idem-OWN-20260422-001 which was already used for a completed transfer with reference PAY-10221
When: transfer is invoked
Then: the stored outcome "transfer confirmed, reference PAY-10221" is returned without contacting the bank, and an audit entry records "duplicate request, stored outcome returned"
