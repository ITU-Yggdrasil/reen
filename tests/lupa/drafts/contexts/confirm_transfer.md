# Context: ConfirmTransfer

## Purpose
Verifies and claims a confirmation card before executing a chat-initiated transfer. The confirmation card is a single-use, time-limited, signed token that represents a transfer the chat agent proposed. The customer must explicitly confirm it before execution proceeds.

## Role Players
- authenticated_customer: AuthenticatedCustomer — the verified customer identity who must match the card's intended customer
- confirmation_card: ConfirmationCard — the signed, single-use token describing the proposed transfer
- ledger_entry: IdempotencyLedgerEntry — the record used to claim the card and prevent double-confirmation

## Role Methods
### confirmation_card
- verify_signature: checks that the card's cryptographic signature is intact and was issued by the system
- verify_freshness: checks that the card has not expired (cards are valid for a limited window)
- verify_customer_match: confirms the card was issued for the same customer who is now confirming it
- provide_transfer_details: supplies the transfer parameters (source, destination or recipient, amount, message, transfer kind) embedded in the card

### ledger_entry
- claim_card: attempts to claim the confirmation card's unique identifier, preventing a second confirmation of the same card

## Functionalities
### confirm
The confirmation_card's verify_signature confirms integrity. The confirmation_card's verify_freshness confirms the card has not expired. The confirmation_card's verify_customer_match confirms the authenticated customer matches the card's intended customer. The ledger_entry's claim_card ensures the card has not already been claimed. If all checks pass, the confirmation_card's provide_transfer_details supplies the transfer parameters for execution. If any check fails, the confirmation is rejected with the specific reason.

## Scenarios
### valid confirmation card is claimed and transfer details returned
Given: authenticated customer C-9281 presents confirmation card CARD-7829 which was issued 90 seconds ago for customer C-9281, proposing a domestic transfer of 89.90 EUR to recipient 1234-56789012 with message "Invoice 2026-04-001", the card's signature is valid, and CARD-7829 has not been claimed
When: confirm is invoked
Then: the card passes signature, freshness, and customer checks, the ledger entry claims CARD-7829, and the transfer details are returned for execution

### expired confirmation card is rejected
Given: authenticated customer C-9281 presents confirmation card CARD-7830 which was issued 420 seconds ago (beyond the freshness window of 300 seconds)
When: confirm is invoked
Then: the confirmation is rejected with reason "confirmation card expired", no claim is made, and no transfer details are returned

### confirmation card already claimed
Given: authenticated customer C-9281 presents confirmation card CARD-7829 which has already been claimed
When: confirm is invoked
Then: the confirmation is rejected with reason "confirmation card already used" and no transfer details are returned

### customer mismatch is rejected
Given: authenticated customer C-4455 presents confirmation card CARD-7829 which was issued for customer C-9281
When: confirm is invoked
Then: the confirmation is rejected with reason "customer does not match confirmation card" and no transfer details are returned
