# Ledger

## Description

The ledger is the cvore of the banking system. Any transaction is written to the ledger in form of LedgerEntries. When they are comitted to the ledger, the ledger records the LedgerEntry. 

The ledger is immutable so adding a new ledger entry produces a new ledger object. The entries of the ledger are kept.


## Properties

- **head** a ledger entry
- **tail** an immutable reference to a ledger object representing all previous ledger entries or None if head is the first ever entry

## Functionality  

**get_entries_for** Provided an account number (i32) the ledger returns all ledger entries where that account is either sink or source. It should be sorted ascending based on the timestamps of the entries. Duplicate timestampts can't happen

**add_entry** Commits an entry to the ledger. The result of the operation is a _new_ ledger, with the old one as it's (internal) tail. Every ledger entry represents a transaction. Adding the entry, is comparable to comitting an atomic transaction in a database.

**new** Accepts an entry and creates a new ledger with None as the tail and the provided entry as the head
**settle** Only valid for an unsettled entry i.e. one where the sink is None. Since the entries are immutable, the method creates a new entry based on the input/argument setting the sink to the provided account id, as well as setting prev_hash_sink. If a previous ledger entry for the sink is not present on the ledger. The prev_hash_sink is set to None.
The timestamp is the timestamp of the original ledger entry.
Returns anyhow::Result<LedgerEntry>

**create_entry** provided with a source (including None), an amount, the method constructs a new ledger entry and returns this.
- sink is always None at this point for the ledger entry
- timestamp is utc.now
- if source is not None, then the hash of the most recent transaction for the source account must be used for prev_hash_source. If source is None then prev_hash_source is also None
- the hash is not provided, it's calculated by the entry itself
Returns anyhow::Result<LedgerEntry>
