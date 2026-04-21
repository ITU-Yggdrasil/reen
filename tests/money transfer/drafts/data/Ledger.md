# Data: Ledger

## Purpose

The Ledger is the core of the banking system. Every transaction is recorded as a LedgerEntry. The ledger is immutable — adding or modifying an entry always produces a new Ledger value, preserving the full history through a chain of previous states.

## Attributes

- head: the most recent ledger entry
- tail: the previous Ledger, or absent for the very first entry

## Invariants

- The ledger is immutable; operations produce new Ledger values rather than modifying existing ones.
- The hash chain is preserved: each entry's prev_hash matches the head hash of the preceding ledger state.

## Operations

### new
Creates a new Ledger with a single entry as the head and no tail.

### get_entries_for
Given an account identifier, returns all ledger entries where that account appears as either source or sink. The result is sorted ascending by timestamp.

### add_entry
Commits an entry to the ledger. Returns a new Ledger whose head is the new entry and whose tail is the previous Ledger. At least one of sink or source must be present on the entry, and the entry's prev_hash must match the current head's hash.

### settle
Takes an unsettled entry (one whose sink is absent) and an account identifier. Returns a new entry with the sink set to the given account, preserving the original timestamp.

### create_entry
Given an optional source account and an amount, constructs a new unsettled ledger entry. If a source is provided, at least one entry for that account must already exist on the ledger. The new entry has sink absent, the current UTC time as its timestamp, the current head's hash as its prev_hash, and a hash computed by the entry itself.

## Scenarios

### add an entry to the ledger
Given: a Ledger with one initial entry for account 123456
When: add_entry is called with a valid new entry whose prev_hash matches the head
Then: a new Ledger is returned with the new entry as head and the old Ledger as tail

### retrieve entries for an account
Given: a Ledger with entries for accounts 123456 and 654321
When: get_entries_for is called for account 123456
Then: only the entries involving account 123456 are returned, sorted ascending by timestamp

### settle an unsettled entry
Given: an unsettled entry (sink is absent) with source 123456 and amount 250.00 DKK
When: settle is called with account 654321
Then: a new entry is returned with sink 654321 and the original timestamp preserved

### create an unsettled entry
Given: a Ledger where account 123456 has at least one entry
When: create_entry is called with source 123456 and amount 250.00 DKK
Then: a new unsettled entry is returned with sink absent, the current time, and prev_hash matching the current head
