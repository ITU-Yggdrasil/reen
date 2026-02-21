1. Description
The ledger records transactions as LedgerEntries. When an entry is committed to the main ledger, the ledger stores the LedgerEntry together with the datetime (UTC) at which it was committed. The ledger is immutable: adding a new ledger entry produces a new ledger object that keeps all previous entries.

2. Type Kind (Struct / Enum / NewType / Unspecified)
Struct

3. Mutability (Immutable / Mutable)
Immutable

4. Properties (only those explicitly mentioned)
- head: A tuple consisting of:
  - a ledger entry
  - the datetime (UTC) when that entry was committed
- tail: An immutable reference to a ledger object representing all previous ledger entries, or None if head is the first ever entry

5. Functionalities (only those explicitly named)
- get_entries_for:
  - Input: an account number
  - Behavior: returns all ledger entries where the provided account is either sink or source
  - Sorting: results are sorted based on the timestamps of when the entries were committed
- add_entry:
  - Input: an entry
  - Behavior: commits the entry to the main ledger and records when it was added using datetime.now in utc
  - Result: a new ledger whose head contains the committed entry and its commit datetime, with the prior ledger as its internal tail; the prior ledger remains unchanged
  - Note: every ledger entry represents a transaction; adding the entry is comparable to committing an atomic transaction in a database
- new*:
  - Input: an entry
  - Behavior: creates a new ledger with None as the tail and the provided entry as the head

6. Constraints & Rules (only those explicitly stated or directly implied)
- Any transaction is written to the ledger in the form of a LedgerEntry.
- When an entry is committed to the main ledger, the ledger records both the LedgerEntry and the datetime (UTC) of commitment.
- The ledger is immutable: operations that add entries produce new ledger objects; previous entries are kept.
- get_entries_for returns only entries where the specified account is either sink or source and sorts them by the commit timestamps.

Inferred Types or Structures (Non-Blocking)
- Location: head
  - Inference made: tuple-like structure of (LedgerEntry, datetime-UTC)
  - Basis for inference: “a tuple of an entry and when it was committed” and mention of “datetime (UTC) of when it was committed”
- Location: tail
  - Inference made: optional/reference-like shape Option<Ledger>
  - Basis for inference: “or None if head is the first ever entry”
- Location: get_entries_for result
  - Inference made: list-like collection of LedgerEntries
  - Basis for inference: “returns all ledger entries … It should be sorted …”

Unspecified or Ambiguous Aspects
- The concrete data type/format for “datetime (UTC)” is unspecified.
- The concrete representation/type of the “account number” parameter is unspecified.
- The sort direction for get_entries_for (ascending vs. descending) and stability on equal timestamps are unspecified.
- For new*: how the commit datetime for head is determined (e.g., whether it uses “datetime.now in utc” at creation) is unspecified.
- The exact semantics of “immutable reference” for tail (ownership, aliasing, lifetime) are unspecified.