# LedgerEntry

## Description

A ledger entry is an entry in the main ledger. It has a source account, a destination account, and an amount

The source might be None signifying that it's a cash deposit, the sink would on the other hand be None if it's a cash withdrawal

If a transfer is reflected by the ledger entry, then both sink and source will be Some(...). 
The amount must be greater than 0.

## Properties

- **sink:** Option<i32>
- **source:** Option<i32>
- **amount:** : type is amount and the amount must be larger than 0
- **timestamp:** DateTime<Utc> - The timestamp (utc.now) of when the ledger entry was created.
- **prev_hash_source** : Option<string> The hash of the most recent transaction - as determined by the timestamp - on the ledger for the source account. This shall be None if source is None. If the source account is not None then this cannot be None either.
- **prev_hash_sink** : Option<string> The hash of the most recent transaction - as determined by the timestamp - on the ledger for the sink account. This might be None even if a sink account is specified.
- **hash**: String -  a SHA256 calculated on a json serialization of all other fields. The json serialisation should be handle by the auto-implementation from serde. The fields must be in the order they are listed above

### note on hashes

All hashes MUST represented as strings and be encoded using Base64 as defined in RFC 4648 §4 (the ‘base64’ alphabet A–Z a–z 0–9 + /) and MUST include = padding. No line breaks or whitespace are permitted.

## business rules
- at least one of sink and source must be not None
- the amount must always be larger than 0
- if source is None then prev_hash_source shall also be None, conversely if source is not None then prev_hash_source can't be None either

## Functionality
- **to_str:** {date use ISO 8601 as the format. Example: 2026-02-21T14:35:00Z} - {source id or none} - {sink id or none}:  {amount.to_str()}

- **create:** anyhow::Result<LedgerEntry> - should set all fields. The constructor should however be pub(crate) since it's going to be called from a factory method on the Ledger type. The hash is calculated and is as the only value not provided as an argument. If business ruless are violated an Error is returned and the message would be detailing what rule(s) have been violated