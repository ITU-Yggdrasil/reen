# Data: LedgerEntry

## Purpose

A LedgerEntry is a single immutable record on the ledger. It represents a movement of funds — a deposit (source is absent), a withdrawal (sink is absent), or a transfer (both present). Each entry carries a cryptographic hash that chains it to the previous entry, ensuring tamper-evident ordering.

## Attributes

- sink: the destination account identifier, or absent for a withdrawal
- source: the source account identifier, or absent for a deposit
- amount: the Amount transferred by this entry
- timestamp: the UTC date and time the entry was created
- prev_hash: the hash of the previous entry in the chain, or absent for the very first entry
- hash: a stable SHA-256 hash of this entry's payload, encoded as Base64 (RFC 4648 section 4, with = padding, no whitespace)

## Invariants

- The amount must be greater than zero.
- At least one of sink or source must be present for the entry to be accepted onto the ledger.
- All hashes are Base64-encoded strings using the alphabet A-Z a-z 0-9 + / with = padding and no whitespace.

## Operations

### new
Creates a new ledger entry. All attributes except the hash are provided as arguments; the hash is computed from the entry's payload using SHA-256. If any business rule is violated, an error is returned describing which rule was broken.

### to_str
Returns a human-readable string in the format: "{timestamp} - {source} - {sink}: {amount}"

### get_amount
Returns the amount of this entry.

### get_source
Returns the source account identifier.

### get_sink
Returns the sink account identifier.

### get_timestamp
Returns the timestamp of this entry.

### get_prev_hash
Returns the hash of the previous entry in the chain.

### get_hash
Returns this entry's hash.

## Scenarios

### create a valid entry
Given: source is 123456, sink is absent, amount is 1000.00 DKK, timestamp is a UTC time, prev_hash is absent
When: new is called
Then: a LedgerEntry is returned with a computed SHA-256 hash

### reject zero amount
Given: source is 123456, sink is absent, amount is 0.00 DKK
When: new is called
Then: an error is returned indicating the amount must be greater than zero

### display an entry
Given: a LedgerEntry with source 123456, sink 654321, amount 250.00 DKK, and a timestamp
When: to_str is called
Then: the result follows the format "{timestamp} - 123456 - 654321: 250.00 DKK"
