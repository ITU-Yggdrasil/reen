1. Description
The ledger is the core append-only record of transactions for the banking system. Each transaction is represented by a LedgerEntry. The ledger is immutable: adding a new ledger entry does not modify an existing ledger object; it yields a new ledger object whose tail points to the prior ledger object. Entries already recorded on a ledger are preserved.

2. Type Kind (Struct / Enum / NewType / Unspecified)
Struct

3. Mutability (Immutable / Mutable)
Immutable

4. Properties (only those explicitly mentioned)
- head: a LedgerEntry
- tail: an immutable reference to a ledger object representing all previous ledger entries, or None if head is the first ever entry

5. Functionalities (only those explicitly named)
- get_entries_for
  - Input: account number (i32)
  - Output: all ledger entries where the provided account is either sink or source, sorted ascending by the timestamps of the entries
  - Notes: Duplicate timestamps “can't happen” (so sorting is well-defined without ties)

- add_entry
  - Input: an entry (LedgerEntry)
  - Behavior: Commits the entry to the main ledger by producing a new ledger whose head is the provided entry and whose internal tail is the prior ledger. This operation is comparable to committing an atomic transaction in a database.
  - Output: a new ledger (the original ledger remains unchanged)

- new
  - Input: an entry (LedgerEntry)
  - Behavior: Constructs a new ledger with tail = None and head = the provided entry
  - Output: a new ledger

- settle
  - Valid only for an unsettled entry (an entry whose sink is None)
  - Input: an unsettled LedgerEntry and a provided account id
  - Behavior: Creates a new entry based on the input entry, setting:
    - sink to the provided account id
    - prev_hash_sink accordingly; if a previous ledger entry for the sink is not present on the ledger, prev_hash_sink is set to None
  - Output: anyhow::Result<LedgerEntry>

- create_entry
  - Input: a source (including None) and an amount
  - Behavior: Constructs a new ledger entry and returns it. The following must hold:
    - The prev_hash_source must match the hash of the previous ledger entry for the source account; if source is None then prev_hash_source should also be None.
  - Output: anyhow::Result<LedgerEntry>

6. Constraints & Rules (only those explicitly stated or directly implied)
- The ledger is immutable. Adding a new ledger entry produces a new ledger object. Existing entries are kept.
- The tail of a ledger is None when the head is the first ever entry; otherwise, tail is an immutable reference to the previous ledger object.
- get_entries_for returns entries where the given account is either sink or source, ordered ascending by entry timestamp; duplicate timestamps “can't happen” for the purpose of this ordering.
- settle is only valid when the provided entry’s sink is None. It must set prev_hash_sink to None if no previous ledger entry exists for the sink account on the ledger.
- create_entry must ensure prev_hash_source matches the hash of the immediately preceding ledger entry for the source account; if source is None, prev_hash_source must be None.

Inferred Types or Structures (Non-Blocking)
- Property: tail
  - Inference: Optional wrapper around a reference-like handle to a Ledger (None or a reference)
  - Basis: “an immutable reference to a ledger object … or None if head is the first ever entry”

- Method: get_entries_for return value
  - Inference: List/sequence of LedgerEntry
  - Basis: “returns all ledger entries”

- Method: settle parameter “provided account id”
  - Inference: i32
  - Basis: LedgerEntry.sink is Option<i32>; account numbers elsewhere are i32

- Method: create_entry parameter “source (including None)”
  - Inference: Option<i32> for source
  - Basis: “including None” and LedgerEntry.source is Option<i32>

Blocking Ambiguities
- create_entry field completion
  - The method’s inputs specify only source (including None) and amount. LedgerEntry requires additional fields (at least sink, timestamp, prev_hash_sink, hash). The specification does not state:
    - What value sink should take (e.g., must it be None, making the entry “unsettled”?)
    - How timestamp is determined
    - How prev_hash_sink is determined (if any)
    - How hash is computed timing-wise relative to timestamp selection
  - Impact: Implementers cannot construct a valid LedgerEntry without assumptions about these values.

- settle timestamp handling
  - The method “creates a new entry based on the input/argument,” but does not specify how the timestamp of the new entry is determined (copied from the input entry vs. newly assigned).
  - Impact: This affects ordering by timestamp and the determination of “most recent transaction,” which in turn affects prev_hash* semantics.

- “main ledger” scope/identity
  - add_entry refers to committing to “the main ledger,” but the specification does not define how the main ledger is identified or updated in the broader system (e.g., how the returned new ledger becomes “the main” one).
  - Impact: Externally observable system behavior (which ledger is authoritative) is unclear without additional context.

Implementation Choices Left Open
- Non-blocking: Concrete collection type for the sequence returned by get_entries_for (e.g., vector, list)
- Non-blocking: Mechanism for representing an “immutable reference” to the previous ledger (pointer, handle, persistent structure)
- Non-blocking: Internal storage and traversal strategy for finding “previous ledger entry” for an account (e.g., scan via tail chain vs. indexes)
- Non-blocking: Error typing/details inside anyhow::Result (error variants/messages are not specified)
- Non-blocking: How the system tracks or updates which ledger instance is the “main ledger” after add_entry (outside the Ledger type’s own behavior)