# Money transfer

## Description

THe money transfer is a context where two accounts play the roles of sink and source. An amount will be transferred from the source to the sink.
The money transfer context represents a menoy transfer from the source to the sink. 


## Role Players

- **source**: the account that serves as the source of the funds to be transferred
  - Must have an `account_id` field
- **sink**: the account that serves as the destination for the funds to be transferred
  - Must have an `account_id` field

## Props:
- **amount**: the amount to be transferred between the accounts
- **ledger**: the ledger of the system (Ledger type)

## Role Methods:

### source:

- **withdraw**
  Creates a ledger entry using the ledger prop, The created entry represent a withdrawal from the source account of the amount spcified by the amount prop
  - **Business rules**:
    - that the transferred amount can't exceed the present balance of the source account
  If the transfer can't be completed due to violation of business rules a proper error should be returned. otherwise a LedgerEntry is returned
  
    
### sink

- **deposit**
  uses the ledger prop to settle the ledger entry passed as an argument. The resulting ledger entry now represents a transfer from source to sink of the specified amount. The ledger entry is returned as the result of the operation

## Functionality
- **new**: receives the account id for sink and source (and construct the corresponding account objects). The amount to transfer and the ledger are also passed.
  if the business rules are not met or the construction of the account objects fail an error should be returned otherwise a money transfer context is returned

  - **Business rules**:
    - the currency of the amount must match the currency of the source

- **Transfer**
  Executes the transfer by calling withdraw and deposit and then adding the resulting ledger entry to the ledger.

  - **Workflow**:
    if any operation fails an error should be returned immediately
    1. Call source.withdraw
    2. If withdraw succeeds call sink.deposit with the result of the withdraw
    3. If deposit succeeds, add the returned entry to the ledger
    4. If adding the ledger entry to the ledger succeeds return the resulting ledger of that operation as the result of transfer
    
  