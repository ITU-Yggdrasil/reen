1. Description
A ledger entry representing one transaction recorded in the main ledger. It has a source account, a destination (sink) account, and an amount. The source may be None to signify a cash deposit; the sink may be None to signify a cash withdrawal. If the ledger entry reflects a transfer, both sink and source are Some(...).

2. Type Kind (Struct / Enum / NewType / Unspecified)
Struct

3. Mutability (Immutable / Mutable)
Immutable

4. Properties (only those explicitly mentioned)
- sink: Option<i32>
  - Destination account identifier. May be None. If None, the entry signifies a cash withdrawal.
- source: Option<i32>
  - Source account identifier. May be None. If None, the entry signifies a cash deposit.
- amount: amount
  - The transacted amount. The amount must be larger than 0.
- timestamp: DateTime<Utc>
  - The timestamp (utc.now) of when the ledger entry was created.
- prev_hash_source: Option<string>
  - The hash of the most recent transaction—as determined by the timestamp—on the ledger for the source account. This shall be None if source is None. If source is not None then this cannot be None.
- prev_hash_sink: Option<string>
  - The hash of the most recent transaction—as determined by the timestamp—on the ledger for the sink account. This might be None even if a sink account is specified.
- hash: String
  - A SHA256 calculated on a JSON serialization of all other fields. The JSON serialization should be handled by the auto-implementation from serde. The fields must be serialized in the order they are listed above (sink, source, amount, timestamp, prev_hash_source, prev_hash_sink).

5. Functionalities (only those explicitly named)
- to_str: {date use ISO 8601 as the format. Example: 2026-02-21T14:35:00Z} - {source id or none} - {sink id or none}:  {amount.to_str()}
  - Formats a string that includes:
    - date in ISO 8601 (example: 2026-02-21T14:35:00Z)
    - source id or the literal "none"
    - sink id or the literal "none"
    - the result of amount.to_str()
- create: anyhow::Result<LedgerEntry>
  - Should set all fields.
  - The constructor should be pub(crate).
  - The hash is calculated and is the only value not provided as an argument.
  - If business rules are violated, returns an Error with a message detailing which rule(s) have been violated.

6. Constraints & Rules (only those explicitly stated or directly implied)
- At least one of sink and source must be not None.
- The amount must always be larger than 0.
- If source is None then prev_hash_source shall also be None; if source is not None then prev_hash_source cannot be None.
- For hashing:
  - hash is the SHA256 over the JSON serialization of [sink, source, amount, timestamp, prev_hash_source, prev_hash_sink] in exactly that order.
  - All hashes (prev_hash_source, prev_hash_sink when present, and hash) MUST be represented as strings and be encoded using Base64 as defined in RFC 4648 §4 (the base64 alphabet A–Z a–z 0–9 + /) and MUST include = padding. No line breaks or whitespace are permitted.

Inferred Types or Structures (Non-Blocking)
- Location: to_str (method)
  - Inference made: Return type anyhow::Result<&str>.
  - Basis for inference: Allowed convention for methods named to_str where no explicit return type is provided.

Unspecified or Ambiguous Aspects
- Representation details for the JSON serialization beyond “serde auto-implementation” and the mandated field order are not specified (e.g., whitespace, key naming/style, numeric formatting of amount’s internals).
- The exact string type behind “string” in Option<string> is not further defined beyond being a string (e.g., whether this corresponds to Rust’s String type is not stated in the draft text itself).
- Validation of sink/source numeric values (e.g., whether negative values are allowed) is not specified here. Only their optionality and the coupling with prev_hash_source are specified.

Worth to Consider
- Non-blocking, out-of-scope: Canonicalization of JSON for hashing (e.g., ensuring stable serializer configuration) to guarantee identical hashes across environments.
- Non-blocking, out-of-scope: Aligning sink/source identifier constraints with the Account context (e.g., positive integers) if that is a system-wide rule.