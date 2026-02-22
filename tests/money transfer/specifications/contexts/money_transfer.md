# Money transfer

## Description
The Money transfer is a context where two accounts play the roles of sink and source. An amount will be transferred from the source to the sink.

## Role Players
- source: the account that serves as the source of the funds to be transferred
  - Must have an account_id field
- sink: the account that serves as the destination for the funds to be transferred
  - Must have an account_id field

## Props
- amount: the amount to be transferred between the accounts
- ledger: the main ledger of the system (Ledger type)

## Role Methods

### source
- withdraw
  - Creates a ledger entry for the source account with the amount specified by the amount prop.
  - Returns: anyhow::Result<LedgerEntry>

### sink
- deposit
  - Uses the ledger to settle the ledger entry created by the withdrawal operation. That way, a ledger entry representing the complete transfer is created.
  - Returns: anyhow::Result<LedgerEntry>

## Functionality

- new
  - Receives the account id for sink and source (and constructs the corresponding account objects). The amount to transfer and the ledger are also passed. If no relevant business rules are violated, Money Transfer context is returned; otherwise, an error is returned.

- Transfer
  - Executes the transfer by calling withdraw and deposit and then adding the resulting ledger entry to the main ledger.
  - Workflow:
    1. Call source.withdraw.
    2. If withdraw succeeds, call sink.deposit.
    3. If deposit succeeds, add the returned entry to the ledger.
    4. If adding the ledger entry to the ledger succeeds, result the newly created ledger.
  - Business rules: If the transfer cannot be completed due to violation of business rules, a proper error should be returned. Business rules include:
    - All business rules that apply to an account.
    - That the transferred amount cannot exceed the present balance of the source account.

## Inferred Types or Structures (Non-Blocking)
- Location: Props.amount
  - Inference: amount is the data type named amount (as defined in the referenced dependency), not a plain numeric.
  - Basis: The draft references a data type called amount in the direct dependencies; Money transfer uses “amount” as a named prop consistently.

- Location: Role Methods → source.withdraw result
  - Inference: The LedgerEntry created by withdraw is an “unsettled” entry with sink == None.
  - Basis: sink.deposit is described as using the ledger to “settle the ledger entry created by the withdrawal operation,” which matches the Ledger.settle behavior that requires an entry where sink is None.

- Location: Functionality → Transfer return value
  - Inference: Transfer returns a newly created Ledger (i.e., the result of adding the ledger entry to the ledger), wrapped in a success result.
  - Basis: Workflow step 4 states “result the newly created ledger” after adding the entry; Ledger.add_entry returns a new ledger.

- Location: Role Players → source.account_id and sink.account_id
  - Inference: account_id is an integer (i32).
  - Basis: The Ledger dependency uses i32 for account numbers (get_entries_for), and the Account dependency describes account_id as a positive integer. The conventional integer default in the dependencies is i32.

## Blocking Ambiguities
- Functionality → Transfer return type
  - The exact return type is not explicitly specified. The workflow implies returning the newly created ledger, but the signature and error type are not defined. This affects the externally observable behavior of the Transfer function.

- Role Methods → sink.deposit inputs
  - It is not specified how sink.deposit receives or identifies “the ledger entry created by the withdrawal operation.” Whether it is passed as an argument, retrieved from state, or otherwise obtained is not defined, which affects the externally observable method contract.

- Functionality → new return type
  - The exact return type for new is not specified (e.g., whether it returns a result type and what the success value is named/typed), beyond stating that either “Money Transfer context is returned” or “an error is returned.” This affects the externally observable behavior of new.

## Implementation Choices Left Open (Non-Blocking)
- Internal representation of the Money transfer context object.
- How the created ledger (from adding the entry) is made available beyond the function return (if any).
- Error value structure and message formatting, provided that errors are returned when business rules are violated (the draft does not mandate a specific error taxonomy or format).
- Concrete mechanics for passing or referencing the withdrawal-created ledger entry into sink.deposit, as long as the behavior matches the described workflow and externally observable outcomes.