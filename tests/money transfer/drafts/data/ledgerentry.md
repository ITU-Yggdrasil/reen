# LedgerEntry

## Description

A ledger entry is an entry in the main ledger. It has a source account, a destination account, and an amount

The source might be None signifying that it's a cash deposit, the sink would on the other hand be None if it's a cash withdrawal

If a transfer is reflected by the ledger entry, then both sink and source will be Some(...). 

## Fields

| Field | Meaning | Notes |
|---|---|---|
| sink | Optional destination account id | `None` for cash withdrawal |
| source | Optional source account id | `None` for cash deposit |
| amount | Amount transferred by the entry | Must be larger than `0` |
| timestamp | UTC timestamp of the entry | Uses `chrono::DateTime<Utc>` |
| prev_hash | Hash of the previous entry | `None` for the first-ever entry |
| hash | Stable SHA256 hash of the entry payload | Represented as Base64 text |

## Rules

All hashes MUST represented as strings and be encoded using Base64 as defined in RFC 4648 §4 (the ‘base64’ alphabet A–Z a–z 0–9 + /) and MUST include = padding. No line breaks or whitespace are permitted.
- the amount must always be larger than 0
- sink and source may both be `None`, but the ledger rejects such an entry when adding it.

## Functionalities
- **to_str** returns a string with the following format: `{timestamp} - {source} - {sink}:  {amount}`
- **new** The hash is calculated and is as the only value not provided as an argument. If business ruless are violated an Error is returned and the message would be detailing what rule(s) have been violated
- **get_amount**: returns the amount object of the entry
- **get_source**: returns the source
- **get_sink**: returns the sink
- **get_timestamp**: returns the timestamp
- **get_prev_hash**: returns the prev_hash
- **get_hash**: returns the hash
