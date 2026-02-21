# Money transfer

## Description
The money transfer is a context where two accounts play the roles of sink and source. An amount is transferred from the source to the sink.

## Role Players
- source: the account that serves as the source of the funds to be transferred
  - Must have an account_id: String field
- sink: the account that serves as the destination for the funds to be transferred
  - Must have an account_id: String field

## Props
- amount: the amount to be transferred (f64, positive)
- currency: The currency to be transferred (Currency enum, not String)
- ledger: the main ledger of the system (Ledger type)

## Role Methods

### source
- withdraw
  - Returns: Result<LedgerEntry, String>
  - Purpose: Creates a ledger entry for the withdrawal from the source account

### sink
- deposit
  - Parameters: entry: LedgerEntry, ledger: &Ledger
  - Returns: Result<LedgerEntry, String>
  - Purpose: Receives the ledger entry with the sink set to None and settles the transaction (by means of the ledger entry method)

## Functionality

- Transfer
  - Execute: MoneyTransfer::execute(self) -> Result<Ledger, String>
  - Workflow:
    1. Call source.withdraw(amount, currency_str, &ledger) which returns Result<LedgerEntry, String>
    2. If withdraw succeeds, add the returned entry to the ledger: ledger = ledger.add_entry(withdraw_entry)
    3. Create an unsettled entry for the deposit (or construct it based on the withdraw entry)
    4. Call sink.deposit(unsettled_entry, &ledger) which returns Result<LedgerEntry, String>
    5. If deposit succeeds, add the returned entry to the ledger: ledger = ledger.add_entry(deposit_entry)
    6. Return Ok(ledger) with the updated ledger containing both entries
  - Purpose: Executes the transfer by calling withdraw and deposit and then adding both ledger entries to the main ledger. The business rules/invariants should be delegated to the role methods. This should be a clear implementation of the business logic/use case.
  - Business rules: If the transfer can't be complete due to violation of business rules a proper error should be returned. Business rules include:
    - All business rules that apply to an account including
      - monthly transfer limits for the source account that is the amount must be less than or equal to remaining_monthly_limit
    - that the transferred amount can't exceed the present balance of the source account
    - that the sink and source must have the same currency, this should be check when trying to construct the money trasfer context

Inferred Types or Structures (Non-Blocking)
- Location: Role Methods > sink > deposit; Functionality > Transfer > Workflow step 3
  - Inference: unsettled entry refers to a LedgerEntry with sink set to None
  - Basis: The deposit method states it receives a ledger entry “with the sink set to None” and “settles the transaction,” which aligns with the Ledger Entry settle description where unsettled means sink is None.
- Location: Mentions of None for sink/source in relation to LedgerEntry
  - Inference: None denotes the absence of a value in an Option-like type for sink/source
  - Basis: Conventional meaning of None and the referenced Ledger Entry properties (sink: Option<integer>, sourc:e Option<integer>) in the direct dependency context

Unspecified or Ambiguous Aspects
- source.withdraw parameters
  - The Role Methods section specifies only the return type and purpose. The Workflow calls source.withdraw(amount, currency_str, &ledger). The definitive parameter list and types for withdraw are not specified in Role Methods.
- currency type mismatch in withdraw call
  - Props.currency is “Currency enum, not String,” while the Workflow calls source.withdraw with currency_str. The expected type for the currency argument in withdraw is ambiguous.
- account_id type mismatch
  - Role Players require account_id: String for source and sink. The direct dependency context “Account” states account_id is a positive integer. The authoritative type to use within this context is ambiguous.
- Construction of unsettled_entry (Workflow step 3)
  - It states “Create an unsettled entry for the deposit (or construct it based on the withdraw entry).” The exact construction method, required fields, and whether it must be derived from withdraw_entry or created independently are not specified.
- Withdraw output structure regarding sink/source
  - The deposit method expects an entry “with the sink set to None,” but it is not specified whether source.withdraw returns such an entry or whether additional transformation is needed before deposit.
- Amount representation mismatch
  - Props.amount is f64 (positive). The direct dependency context for Ledger Entry specifies amount is an integer representing 1/100 of the currency unit. Conversion or mapping between f64 and the integer minor unit for LedgerEntry is not specified.
- Error handling and ledger state on failure paths
  - The Workflow describes successful paths for adding entries and returning Ok(ledger). It does not specify:
    - What is returned if source.withdraw fails.
    - What is returned if sink.deposit fails.
    - Whether the ledger should reflect only the withdraw entry if deposit fails, or any compensating action is expected. The resulting ledger state on failure is not defined.
- Timing of the “same currency” check
  - It is stated that “the sink and source must have the same currency, this should be check when trying to construct the money trasfer context.” The mechanism, inputs, and exact point of this construction-time check are not defined within this context.
- Delegation of business rules to role methods
  - It is stated that business rules/invariants “should be delegated to the role methods,” but which specific rules are enforced by source.withdraw versus sink.deposit is not specified.