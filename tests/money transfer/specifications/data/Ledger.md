1. Description
The Ledger is the core, immutable record of transactions. Each transaction is represented as a LedgerEntry. Committing a new LedgerEntry produces a new Ledger object, preserving all prior entries via an internal chain. The current entry is the head; the remainder of the chain is referenced via tail, which represents all previous ledger entries.

2. Type Kind (Struct)
Struct

3. Mutability (Immutable / Mutable)
Immutable

4. Properties (only those explicitly mentioned)
- head: a LedgerEntry
- tail: an immutable reference to a Ledger object representing all previous ledger entries, or None if head is the first-ever entry

5. Functionalities (only those explicitly named)
- get_entries_for
  - Input: an account number (i32)
  - Output: all ledger entries where the account is either sink or source, sorted ascending by the entries’ timestamps
  - Note: Duplicate timestamps for ledger entries for the same account cannot happen

- add_entry
  - Behavior: Commits an entry to the ledger and returns a new Ledger whose tail is the previous ledger and whose head is the committed entry
  - Constraints:
    - At least one of sink and source must be not None
    - The hash of the current head entry must match the prev_hash of the entry being added
  - Semantics note: Adding the entry is comparable to committing an atomic transaction in a database (conceptual only)

- new
  - Input: an entry
  - Behavior: Creates a new ledger with None as tail and the provided entry as head

- settle
  - Validity: Only valid for an unsettled entry (i.e., one where sink is None)
  - Behavior: Creates a new entry based on the input entry, setting sink to the provided account id; the timestamp of the new entry is equal to the timestamp of the original entry
  - Output: anyhow::Result<LedgerEntry>

- create_entry
  - Inputs: a source (including None), and an amount
  - Behavior: Constructs a new ledger entry and returns it
  - Constraints:
    - If source is not None, at least one entry for that account must exist on the ledger
    - sink is always None for the constructed entry
    - timestamp is utc.now
    - prev_hash must be set to the hash of the current head entry on the ledger
    - The hash is not provided; it is calculated by the entry itself
  - Output: anyhow::Result<LedgerEntry>

6. Constraints & Rules (only those explicitly stated or directly implied)
- Ledger immutability: Adding a new entry produces a new Ledger object; prior entries are preserved
- Chain structure:
  - head is the current ledger entry
  - tail is an immutable reference to a Ledger representing all prior entries, or None if there are none
- get_entries_for sorting: Results are sorted ascending by timestamp; duplicate timestamps for entries concerning the same account cannot occur
- add_entry validation:
  - At least one of sink and source must be not None
  - The hash of the current head entry must match the prev_hash of the entry being added
- create_entry requirements:
  - If source is not None, at least one entry for that account must already exist on the ledger
  - sink is None
  - timestamp is utc.now
  - prev_hash equals the current head entry’s hash
  - The entry’s own hash is computed by the entry and not provided as input
- settle requirements:
  - Only valid if the input entry’s sink is None
  - The new entry’s sink is set to the provided account id
  - The new entry’s timestamp equals the original entry’s timestamp

Inferred Types or Structures (Non-Blocking)
- Property: tail
  - Inference: Optional reference to Ledger (e.g., Option<&Ledger> or equivalent)
  - Basis: “an immutable reference to a ledger object … or None if head is the first ever entry”

- Function: get_entries_for return
  - Inference: List-like collection of LedgerEntry
  - Basis: “returns all ledger entries … sorted ascending”

- Function: create_entry input “source (including None)”
  - Inference: Optional source account identifier (e.g., Option<i32>)
  - Basis: Explicit mention of “including None” for source

Implementation Choices Left Open
- Non-blocking: Exact collection type used for “all ledger entries” (e.g., vector, list, iterator)
- Non-blocking: Exact representation of the immutable reference for tail (language- and runtime-specific)
- Non-blocking: Signature details (parameter names, ownership/borrowing, error variants) so long as the described behavior and constraints are met
- Non-blocking: Concrete time source and library for utc.now, provided it yields a UTC timestamp consistent with the stated behavior
- Non-blocking: Internal traversal and storage mechanics of the ledger chain (beyond head/tail semantics)
- Non-blocking: Sorting implementation details given that timestamps are unique per account as stated (no tie-breaking required)