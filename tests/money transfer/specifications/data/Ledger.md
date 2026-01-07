1. **Description**
   The ledger is the core of the banking system. Any transaction is written to the ledger in the form of LedgerEntries. When they are committed to the main ledger, the main ledger records the LedgerEntry as well as the time of when it was committed.

2. **Type Kind** (Struct)
3. **Mutability** (Immutable)
4. **Properties**
   - **head:** a tuple containing an entry and when it was committed.
   - **tail:** an immutable reference to a ledger object representing all previous ledger entries or `None` if head is the first ever entry.

5. **Functionality**
   - **get_entries_for** (Provided an account number, returns all ledger entries where that account is either source or sink, sorted by the timestamps of when they were committed)
   - **add_entry** (Commits an entry to the main ledger and records when it was committed to the ledger. The result is a new ledger with the old one as its internal tail)

6. **Constraints & Rules**
   - Adding a new LedgerEntry produces a new Ledger object.
   - Entries of the ledger are kept.

7. **Unspecified or Ambiguous Aspects**
   - The specific format and structure of `LedgerEntries`.
   - How the sorting in `get_entries_for` is implemented.
   - What constitutes an "account" in the context of `get_entries_for`.