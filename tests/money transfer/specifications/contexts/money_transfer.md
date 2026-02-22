# Money transfer

## Description

The money transfer is a context where two accounts play the roles of sink and source. An amount is transferred from the source to the sink.
The money transfer context represents a money transfer from the source to the sink.

## Role Players

- source: the account that serves as the source of the funds to be transferred
  - Must have an account_id field
- sink: the account that serves as the destination for the funds to be transferred
  - Must have an account_id field

Note: Role Players are immutable for a context instance once constructed.

## Props

- amount: the amount to be transferred between the accounts
- ledger: the ledger of the system (Ledger type)

Props are immutable for a context instance once constructed.

## Role Methods

### source

- withdraw
  - Description:
    - Creates a ledger entry using the ledger prop. The created entry represents a withdrawal from the source account of the amount specified by the amount prop.
  - Business rules:
    - The transferred amount can't exceed the present balance of the source account.
  - Results:
    - If the transfer can't be completed due to violation of business rules, an error is returned.
    - Otherwise, a LedgerEntry is returned.

### sink

- deposit
  - Description:
    - Uses the ledger prop to settle the ledger entry passed as an argument. The resulting ledger entry now represents a transfer from source to sink of the specified amount.
  - Results:
    - Returns the ledger entry resulting from the settlement.
    - If settlement fails, an error is returned.

## Functionality

- new
  - Inputs:
    - The account id for sink
    - The account id for source
    - The amount to transfer
    - The ledger
  - Behavior:
    - Constructs the corresponding account objects for sink and source from the provided account ids and ledger.
    - Validates business rules.
    - If business rules are not met or the construction of the account objects fails, an error is returned.
    - Otherwise, a money transfer context is returned.
  - Business rules:
    - The currency of the amount must match the currency of the source.

- Transfer
  - Description:
    - Executes the transfer by calling withdraw and deposit and then adding the resulting ledger entry to the ledger.
  - Workflow:
    - If any operation fails, an error is returned immediately.
    - 1. Call source.withdraw.
      2. If withdraw succeeds, call sink.deposit with the result of the withdraw.
      3. If deposit succeeds, add the returned entry to the ledger.
      4. If adding the ledger entry to the ledger succeeds, return the resulting ledger of that operation as the result of Transfer.

Inferred Types or Structures (Non-Blocking)

- Location: Role Methods -> source.withdraw
  - Inference: The created ledger entry represents a withdrawal and, per the referenced LedgerEntry description, a withdrawal corresponds to an entry where sink is None and source is set.
  - Basis: The LedgerEntry draft states that a cash withdrawal has sink as None and explains how transfers/withdrawals are represented.

- Location: Role Methods -> sink.deposit
  - Inference: Settlement uses the ledger’s capability to convert an unsettled entry (with sink None) into one with sink set to the sink account’s id, returning a new LedgerEntry.
  - Basis: The Ledger draft defines settle as valid only for an unsettled entry and returning a LedgerEntry.

- Location: Functionality -> Transfer, step 3–4
  - Inference: “Adding the ledger entry to the ledger” uses the Ledger.add_entry operation and produces a new immutable Ledger that is returned.
  - Basis: The Ledger draft describes immutability and that add_entry returns a new Ledger.

Implementation Choices Left Open

- Error representation (non-blocking): The exact error type and error messaging format for failures in new, withdraw, deposit, and Transfer are not specified.
- Account construction mechanism (non-blocking): The exact way to “construct the corresponding account objects” from account ids and the ledger is not specified beyond using the provided inputs.
- Context state update after Transfer (non-blocking): Whether the context instance’s internal ledger prop is also updated or whether only the new Ledger is returned is not specified; only the externally observable return value (the resulting Ledger) is required.