# Account

## Props
- account_id
  - A positive integer identifying the account.
- Ledger
  - The main ledger of the system associated with the account.

## Functionality

- balance
  - The balance of an account is a calculated property.
  - Definition: sum of the amounts of all transactions on the Ledger where the account_id is the sink minus the sum of the amounts of all transactions on the Ledger where the account_id is the source.

- currency
  - All ledger entries for an account must be in the same currency.
  - The value is either None or the currency of previous ledger entries where the account is source or sink, which would all have the same currency due to this constraint.

- account_id
  - Returns the id of the account.

- transactions
  - Returns all ledger entries related to the account (entries where the account_id is sink or source), sorted by transactions date, descending.

- new
  - Accepts an account id and the Ledger.
  - At least one entry for the account must exist on the Ledger; if not, an Error is returned.
  - If at least one ledger entry exists, a result of an account object is returned.

## Business rules
- currency
  - The currency of an account is immutable. Once it is defined (by the first ledger entry), it can't be changed.

## References to Direct Dependencies (contextual, non-normative)
- Ledger
  - Provides get_entries_for(account number) returning entries where the account is either sink or source, sorted ascending by timestamps.
- LedgerEntry
  - Contains sink, source, amount, and timestamp, among other fields.
- Currency
  - The currency type is an enum of active ISO 4217 currency codes.

Inferred Types or Structures (Non-Blocking)
- Location: Props.account_id
  - Inference: account_id is of type i32 and must be positive.
  - Basis: Ledger.get_entries_for is “provided an account number (i32)”; the draft states “the id of the account is a positive integer.”

- Location: Functionality.currency
  - Inference: Return type is Option<Currency> (either None or some currency).
  - Basis: The draft states “This is either None or the currency of previous ledger entries,” and direct dependency defines a Currency enum.

- Location: Functionality.transactions
  - Inference: Returns a sequence/list of LedgerEntry values sorted by timestamp descending.
  - Basis: The draft says “returns all ledger entries related to the account, sorted by transactions date, descending.” LedgerEntry defines a timestamp; Ledger defines entries and their sort order (ascending) for get_entries_for, implying timestamp is the sortable “transactions date.”

- Location: Functionality.balance
  - Inference: The balance is expressed in the same amount type as LedgerEntry.amount.
  - Basis: The balance is defined as sums of “amount” values from ledger entries; LedgerEntry.amount is the only defined amount type.

- Location: Functionality.new
  - Inference: Returns a Result-like value (Ok with an account object, or Error).
  - Basis: The draft states “If at least one ledger entry exist a result of an account object is returned” and “if not an Error is returned,” matching a conventional Result pattern used elsewhere in the direct dependencies.

Implementation Choices Left Open (Non-Blocking)
- How Account holds Ledger (owning vs referencing, copying vs sharing) is not specified.
- Exact error/result type and error payload for new (e.g., error kind/message) are not specified.
- Concrete collection type returned by transactions (e.g., list/array/iterator) is not specified.
- Exact numeric/precision semantics for amount arithmetic are not specified here (the amount type is referenced from LedgerEntry).
- Mechanism for sorting (stability, tie-breaking) is implementation-defined; Ledger states duplicate timestamps for the same account cannot happen.