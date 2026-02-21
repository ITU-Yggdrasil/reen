# Money transfer

## Description

THe money transfer is a context where two accounts play the roles of sink and source. An amount will be transferred from the source to the sink


## Role Players

- **source**: the account that serves as the source of the funds to be transferred
  - Must have an `account_id: String` field
- **sink**: the account that serves as the destination for the funds to be transferred
  - Must have an `account_id: String` field

## Props:
- **amount**: the amount to be transferred (f64, positive)
- **currency**: The currency to be transferred (Currency enum, not String)
- **ledger**: the main ledger of the system (Ledger type)

## Role Methods:

### source:

- **withdraw**
  - **Returns**: `Result<LedgerEntry, String>`
  - **Purpose**: Creates a ledger entry for the withdrawal from the source account
    

### sink

- **deposit**
  - **Parameters**: `entry: LedgerEntry, ledger: &Ledger`
  - **Returns**: `Result<LedgerEntry, String>`
  - **Purpose**: Receives the ledger entry with the sink set to None and settles the transaction (by means of the ledger entry method)
  

## Functionality

- **Transfer**
  - **Execute**: `MoneyTransfer::execute(self) -> Result<Ledger, String>`
  - **Workflow**:
    1. Call `source.withdraw(amount, currency_str, &ledger)` which returns `Result<LedgerEntry, String>`
    2. If withdraw succeeds, add the returned entry to the ledger: `ledger = ledger.add_entry(withdraw_entry)`
    3. Create an unsettled entry for the deposit (or construct it based on the withdraw entry)
    4. Call `sink.deposit(unsettled_entry, &ledger)` which returns `Result<LedgerEntry, String>`
    5. If deposit succeeds, add the returned entry to the ledger: `ledger = ledger.add_entry(deposit_entry)`
    6. Return `Ok(ledger)` with the updated ledger containing both entries
  - **Purpose**: Executes the transfer by calling withdraw and deposit and then adding both ledger entries to the main ledger. The business rules/invariants should be delegated to the role methods. This should be a clear implementation of the business logic/use case.
  - **Business rules**: If the transfer can't be complete due to violation of business rules a proper error should be returned. Business rules include:
    - All business rules that apply to an account including
      - monthly transfer limits for the source account that is the amount must be less than or equal to remaining_monthly_limit
    - that the transferred amount can't exceed the present balance of the source account
    - that the sink and source must have the same currency, this should be check when trying to construct the money trasfer context