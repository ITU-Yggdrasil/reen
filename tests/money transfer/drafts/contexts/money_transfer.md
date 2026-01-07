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
  - **Parameters**: `amount: f64, currency: &str, ledger: &Ledger`
  - **Returns**: `Result<LedgerEntry, String>`
  - **Purpose**: Creates a ledger entry for the withdrawal from the source account
  - **Implementation**:
    - Validates the currency string and converts it to Currency enum (returns error if invalid)
    - Creates a LedgerEntry using `LedgerEntry::source()` with:
      - source = source account_id
      - sink = "unsettled"
      - amount = -amount (negative for withdrawal)
      - currency = converted Currency enum
      - timestamp = Utc::now()
    - Returns the created LedgerEntry
  - **Validation**: If the account has a currency, the currency should match. The invariants related to the source account should be checked and enforced in this method.

### sink

- **deposit**
  - **Parameters**: `entry: LedgerEntry, ledger: &Ledger`
  - **Returns**: `Result<LedgerEntry, String>`
  - **Purpose**: Receives an unsettled transaction and settles it by creating a new entry with the sink account ID
  - **Implementation**:
    - Validates that the entry has source="unsettled" (returns error "Transaction is already settled" if not)
    - Creates a new LedgerEntry with:
      - source = "unsettled"
      - sink = sink account_id
      - amount = entry.amount (positive for deposit)
      - currency = entry.currency
      - timestamp = entry.timestamp or Utc::now()
    - Returns the settled LedgerEntry
  - **Validation**: The check that the currency matches the currency of the destination should be validated in this method.

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
