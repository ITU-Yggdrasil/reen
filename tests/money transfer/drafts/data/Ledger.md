# Ledger

## Description

The ledger is the cvore of the banking system. Any transaction is written to the ledger in form of LedgerEntries. When they are comitted to the main ledger the main ledger records the LedgerEntry as well as the time of when it was comitted

The ledger is immutable so adding a new ledger entry produces a new ledger object. The entries of the ledger are kept.


## Properties

- **head:** a tuple of an entry and when it was commited.
- **tail:** an immutable reference to a ledger object representing all previous ledger entries or None if head is the first ever entry

## Functionality

**get_entries_for** Provided an account number the ledger returns all ledger entries where that account is either sink or source. It should be sorted based on the timestamps of when they were committed

**add_entry** Commits an entry to the main ledger also recording when it was comitted to the ledger. THe result of the operation is a _new_ ledger, with the old one as it's (internal) tail.

**new*:** Accepts an entry and creates a new ledger with None as the tail and the provided entry as the head