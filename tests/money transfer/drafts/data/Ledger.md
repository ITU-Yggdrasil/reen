# Ledger

## Description

The ledger is the core of the banking system.
Any transaction is written to the ledger in the form of a LedgerEntry.
The ledger is immutable, so adding a new entry produces a new ledger object while preserving
the previous chain of entries.

## Fields

| Field | Meaning | Notes |
|---|---|---|
| head | Most recent ledger entry | Required when a ledger exists |
| tail | Previous immutable ledger or `None` | `None` for the first-ever entry |

## Rules

- Adding an entry produces a new immutable ledger value.
- The historical chain of entries is preserved through `tail`.

## Functionalities

- **get_entries_for** Given an account number, returns all ledger entries where that account is either sink or source. The result is sorted ascending by timestamp.
- **add_entry** Commits an entry and returns a new ledger whose old state becomes the internal tail. At least one of sink and source must be present, and the current head hash must match the entry's `prev_hash`.
- **new** Accepts an entry and creates a new ledger with `None` as the tail and the provided entry as the head.
- **settle** Takes an unsettled entry whose sink is `None`, returns a new entry with sink set to the provided account id, and preserves the original timestamp.
- **create_entry** Given a source account (including `None`) and an amount, constructs a new unsettled ledger entry. If source is not `None`, at least one entry for that account must already exist on the ledger. The created entry always has `sink = None`, `timestamp = utc.now`, `prev_hash` equal to the current head hash, and a hash calculated by the entry itself.
