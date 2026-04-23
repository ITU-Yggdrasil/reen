# Context: EnforceIdempotency

## Purpose
Guards mutating operations against duplicate execution by checking an idempotency token before the operation proceeds. If the token has been seen before, the stored outcome is returned without re-executing the operation.

## Role Players
- authenticated_customer: AuthenticatedCustomer — the identity requesting the operation
- idempotency_token: IdempotencyToken — the client-supplied unique token for this request
- idempotency_ledger: IdempotencyLedger — stores token-to-outcome mappings
- intended_operation: a description of the operation to be guarded (used for verification, not execution)

## Role Methods
### idempotency_ledger
- lookup: checks whether the idempotency token has been recorded previously and, if so, retrieves the stored outcome
- claim: reserves the token for the current operation, preventing concurrent duplicate claims
- record_outcome: stores the final outcome against the token after the operation completes

### idempotency_token
- provide_key: supplies the unique key to look up or claim

## Functionalities
### enforce
The idempotency_token's provide_key supplies the key. The idempotency_ledger's lookup checks for a prior execution. If the token has been seen, the stored outcome is returned immediately and no operation proceeds. If the token is new, the idempotency_ledger's claim reserves it. The caller then executes the operation and calls record_outcome to persist the result. If a concurrent request attempts to claim the same token, only one succeeds — the other receives a conflict indication.

## Scenarios
### first-time token — operation proceeds
Given: authenticated customer C-9281 submits idempotency token idem-TXN-20260422-001 for a transfer of 500.00 DKK
When: enforce is invoked and the idempotency ledger contains no record for idem-TXN-20260422-001
Then: the token is claimed, the result is "first time, proceed", and the caller is expected to execute the operation and record the outcome

### duplicate token — stored outcome returned
Given: authenticated customer C-9281 submits idempotency token idem-TXN-20260422-001 for a transfer of 500.00 DKK
When: enforce is invoked and the idempotency ledger already contains idem-TXN-20260422-001 with stored outcome "transfer confirmed, reference PAY-88712"
Then: the stored outcome "transfer confirmed, reference PAY-88712" is returned and no operation is executed

### concurrent duplicate — one wins, one gets conflict
Given: two simultaneous requests from customer C-9281 both carry idempotency token idem-TXN-20260422-002
When: enforce is invoked concurrently for both requests
Then: exactly one request claims the token and proceeds; the other receives a conflict indication and does not execute
