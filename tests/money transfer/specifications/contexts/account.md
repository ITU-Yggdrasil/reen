# Account

## Props
- account_id: Positive integer. The id of the account.
- Ledger: The main ledger of the system.

## Functionality

- balance:
  - Calculated property.
  - Definition: sum of all transactions on the ledger where the account id is the sink, minus the sum of all transactions on the ledger where the account id is the source.

- currency:
  - All ledger entry for an account must be in the same currency.
  - The value is either None or the currency of previous ledger entries where the account is source or sink. Those previous entries would all have the same currency due to this constraint.

- account_id:
  - The id of the account.

- remaining_monthly_limit:
  - Defined as 100000 minus the amount transferred out of the account within the last 30 days, or zero if the result would otherwise be negative.

## Business rules
- currency:
  - The currency of an account is immutable. Once it is defined (by the first ledger entry), it can't be changed.

Inferred Types or Structures (Non-Blocking)
- currency (Functionality > currency)
  - Inference: Optional value (e.g., Option<currency>), with possible value None or a currency.
  - Basis: The text states “This is either None or the currency of previous ledger entries...”. The direct dependency context defines a currency enum named currency.

- account_id (Props > account_id)
  - Inference: Positive integer type.
  - Basis: The text states “the id of the account is a positive integer”.

- balance (Functionality > balance)
  - Inference: Integer amount in the same nominal unit as ledger entry amounts.
  - Basis: The definition sums ledger entry amounts; the Ledger Entry dependency defines amount as an integer representing 1/100 of the currency unit.

- Ledger (Props > Ledger)
  - Inference: Refers to the Ledger type described in the provided dependency context.
  - Basis: Name match “Ledger” and “the main ledger of the system”.

Unspecified or Ambiguous Aspects
- remaining_monthly_limit
  - Definition of “amount transferred out of the account” is not specified (e.g., whether it strictly means entries where the account id is the source, how to treat cash withdrawals, or any other cases).
  - Time window “within the last 30 days” is not specified regarding:
    - Which timestamp to use (ledger commit time vs. ledger entry timestamp).
    - Inclusivity/exclusivity of boundaries.
    - The exact interpretation of “last 30 days” (rolling 30×24 hours vs. calendar days).
  - Currency and unit for the constant 100000 are not specified, nor whether it is in the same nominal unit as ledger entry amounts.
  - Whether only committed entries on the main ledger are considered is not specified.

- currency initialization
  - The exact moment and mechanism by which the currency becomes defined “by the first ledger entry” are not detailed (e.g., which timestamp/ordering defines “first” if multiple entries exist).

- balance
  - No explicit statement identifies whether any filtering by time or entry state applies beyond “transactions on the ledger”; if any such filtering exists, it is not specified.