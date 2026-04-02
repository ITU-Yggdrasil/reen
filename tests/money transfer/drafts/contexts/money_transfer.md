# Money transfer

## Purpose

The money transfer context represents a transfer of an amount from the source account to the sink account.

## Role Players

| Role player | Why involved | Expected behaviour |
|---|---|---|
| source | Provides the funds to be transferred | Exposes an account id and can withdraw through the ledger |
| sink | Receives the transferred funds | Exposes an account id and can settle a withdrawal into a deposit |

## Role Methods

### source

- **withdraw**
  Creates a ledger entry using the ledger prop.
  The created entry represents a withdrawal from the source account of the amount specified by the amount prop.
  The transferred amount cannot exceed the present balance of the source account.
  If the transfer cannot be completed due to violation of business rules, an error is returned.
  
### sink

- **deposit**
  Uses the ledger prop to settle the ledger entry passed as an argument.
  The resulting ledger entry now represents a transfer from source to sink of the specified amount.

## Props

| Prop | Meaning | Notes |
|---|---|---|
| amount | Amount to transfer between the accounts | |
| ledger | Ledger of the system | Uses the Ledger type |

## Functionalities

### new

| Started by | Uses | Result |
|---|---|---|
| caller creating a transfer | source, sink, amount, ledger | money transfer context is created or an error is returned |

Rules:
- Receives the account ids for sink and source and constructs the corresponding account objects.
- Receives the amount to transfer and the ledger.
- If business rules are not met, or account construction fails, returns an error.
- The currency of the amount must match the currency of the source.

| Given | When | Then |
|---|---|---|
| source and sink accounts exist and amount currency matches the source | new is called | a Money transfer context is returned |

### Transfer

| Started by | Uses | Result |
|---|---|---|
| caller executing the transfer | source, sink, ledger | new ledger is returned or an error is returned |

Rules:
- Executes the transfer by calling withdraw and deposit and then adding the resulting ledger entry to the ledger.
- If any operation fails, returns an error immediately.
- Calls `source.withdraw` first.
- If withdraw succeeds, calls `sink.deposit` with the returned entry.
- If deposit succeeds, adds the returned entry to the ledger.
- If the ledger add succeeds, returns the resulting ledger.

| Given | When | Then |
|---|---|---|
| withdraw, deposit, and add_entry all succeed | Transfer is called | the resulting updated ledger is returned |
