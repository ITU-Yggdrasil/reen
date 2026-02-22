# Account

## Props
- account_id
  - The id of the account.
  - The value is a positive integer.
- Ledger
  - The main ledger of the system used for all calculations and retrievals described below.

## Functionality

- balance
  - Definition: A calculated property.
  - Computation: sum of amounts of all transactions on the Ledger where the account_id is the sink, minus the sum of amounts of all transactions on the Ledger where the account_id is the source.

- currency
  - Constraint: All ledger entries for an account must be in the same currency.
  - Value: Either None or the currency of previous ledger entries where the account is source or sink (which are all the same due to the above constraint).

- account_id
  - Returns the id of the account.

- transactions
  - Returns all ledger entries related to the account (entries where the account_id is either sink or source).
  - Sorting: By transactions date, descending.

- new
  - Accepts an account id and the Ledger.
  - If at least one ledger entry for the account exists on the Ledger, a result of an account object is returned.
  - If no ledger entry for the account exists on the Ledger, an Error is returned.

## Business rules
- currency
  - The currency of an account is immutable. Once it is defined (by the first ledger entry), it can't be changed.

Inferred Types or Structures (Non-Blocking)
- currency (Functionality → currency)
  - Inference: Optional value (None or a currency value), i.e., an option-like type holding a currency value when defined.
  - Basis: The draft explicitly states “This is either None or the currency of previous ledger entries...”
- transactions (Functionality → transactions)
  - Inference: A sequence/list of LedgerEntry items.
  - Basis: The draft says it “returns all ledger entries related to the account,” which conventionally implies a collection of LedgerEntry values.
- transactions sorting key (Functionality → transactions)
  - Inference: “transactions date” corresponds to the timestamp field of ledger entries.
  - Basis: In the direct dependency context, LedgerEntry exposes a timestamp. No other “date” field is defined.

Implementation Choices Left Open (Non-Blocking)
- Exact integer width for account_id
  - The draft specifies “positive integer” while the direct dependency Ledger.get_entries_for uses i32. Implementations may choose a concrete integer type compatible with Ledger.
- Error/result mechanics of new
  - The draft states that an Error is returned when no entries exist and that otherwise a result of an account object is returned, without fixing a concrete error or result type.
- Balance numeric accumulation details
  - The draft defines balance as a difference of sums over amounts but does not fix the concrete numeric/amount type or precision; implementations should use the same semantics as the underlying amount type used by LedgerEntry.
- Retrieval mechanism for transactions
  - The draft requires that transactions be all ledger entries related to the account and sorted descending by transactions date; the mechanism (e.g., using Ledger.get_entries_for and re-sorting) is left to the implementation.