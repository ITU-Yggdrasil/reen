# Ledger

## Description

The ledger is the cvore of the banking system. Any transaction is written to the ledger in form of LedgerEntries. When they are comitted to the main ledger the main ledger records the LedgerEntry. Since the ledger itself is immutable we accept the order they

The ledger is immutable so adding a new ledger entry produces a new ledger object. The entries of the ledger are kept.


## Properties

- **head:** a ledger entry
- **tail:** an immutable reference to a ledger object representing all previous ledger entries or None if head is the first ever entry

## Functionality

**get_entries_for** Provided an account number the ledger returns all ledger entries where that account is either sink or source. It should be sorted based on the timestamps of the entries

**add_entry** Commits an entry to the main ledger. The result of the operation is a _new_ ledger, with the old one as it's (internal) tail. Every ledger entry represents a transaction. Adding the entry, is comparable to comitting an atomic transaction in a database.

**new*:** Accepts an entry and creates a new ledger with None as the tail and the provided entry as the head
**settle:** Only valid for an unsettled entry i.e. one where the sink is None. Since the entries are immutable, the method creates a new entry based on the input/argument setting the sink to the provided account id, as well as setting prev_hash_sink. If a previous ledger entry for the sink is not present on the ledger. The prev_hash_sink is set to None. Returns anyhow::Result<LedgerEntry>

**create_entry:** provided with a source (including None), an amount and a currency, the method constructs a new ledger entry and returns this. 
- the prev_hash_source must match the hash of the previous ledger entry for the source account or if source is None then the prev_hash_source should also be None
Returns anyhow::Result<LedgerEntry>
