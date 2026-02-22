# Money transfer

## Description

THe money transfer is a context where two accounts play the roles of sink and source. An amount will be transferred from the source to the sink


## Role Players

- **source**: the account that serves as the source of the funds to be transferred
  - Must have an `account_id` field
- **sink**: the account that serves as the destination for the funds to be transferred
  - Must have an `account_id` field

## Props:
- **amount**: the amount to be transferred between the accounts
- **ledger**: the main ledger of the system (Ledger type)

## Role Methods:

### source:

- **withdraw**
  Creates a ledger entry for the source account with the amount spcified by the amount prop
  - **Returns**: `anyhow::Result<LedgerEntry>`
    
### sink

- **deposit**
  uses the ledger to settle the ledger entry created by the withdrawl operation. That way a ledger entry representing the complete transfer is created.
  - **Returns**: `anyhow::Result<LedgerEntry>`

## Functionality
- **new**: receives the account id for sink and source (and construct the corresponding account objects). The amount to transfer and the ledger are also passed. If no relevant business rules are violated Money Transfer context is returned otherwise an error is returned.

- **Transfer**
  Executes the transfer by calling withdraw and deposit and then adding the resulting edger entry to the main ledger. 

  - **Workflow**:
    1. Call source.withdraw
    2. If withdraw succeeds call sink.deposit
    3. If deposit succeeds, add the returned entry to the ledger
    4. If adding the ledger entry to the ledger succeeds result the newly created ledger

  - **Business rules**: If the transfer can't be completed due to violation of business rules a proper error should be returned. Business rules include:
    - All business rules that apply to an account
    - that the transferred amount can't exceed the present balance of the source account