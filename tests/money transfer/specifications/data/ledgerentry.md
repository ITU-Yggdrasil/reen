1. Description
A ledger entry representing a single transaction in the main ledger. It records a source account, a destination account (sink), an amount, and related hash linkage. The source may be None to represent a cash deposit. The sink may be None to represent a cash withdrawal. For transfers, both source and sink are Some(...). The amount must be greater than 0.

2. Type Kind (Struct / Enum / NewType / Unspecified)
Struct

3. Mutability (Immutable / Mutable)
Immutable

4. Properties (only those explicitly mentioned)
- sink: Option<i32>
  - Destination account identifier. May be None (e.g., for a cash withdrawal).
- source: Option<i32>
  - Source account identifier. May be None (e.g., for a cash deposit).
- amount: amount
  - A monetary amount. The amount must be larger than 0.
- timestamp: chrono::DateTime<Utc>
  - Timestamp of the ledger entry.
- prev_hash_source: Option<String>
  - The hash of the most recent transaction — determined by timestamp — on the ledger for the source account.
  - Must be None if source is None.
  - Must not be None if source is not None.
  - It is not possible for two ledger entries for the same account to have matching timestamps.
- prev_hash_sink: Option<String>
  - The hash of the most recent transaction — determined by timestamp — on the ledger for the sink account.
  - May be None even if a sink account is specified.
  - It is not possible for two ledger entries for the same account to have matching timestamps.
- hash: String
  - A SHA256 calculated over a UTF-8 encoded string formed by concatenating the values of the type (excluding the hash field). The method of concatenation and ensuring stable, reproducible hashes is an implementation detail.
  - All hashes must be represented as strings and be Base64-encoded per RFC 4648 §4 (alphabet A–Z a–z 0–9 + /) with = padding. No line breaks or whitespace are permitted.

5. Functionalities (only those explicitly named)
- to_str -> anyhow::Result<&str>
  - Returns a string formatted exactly as: "{timestamp:?} - {source:?} - {sink:?}:  {amount:?}".
- create -> anyhow::Result<LedgerEntry>
  - Constructs a LedgerEntry and sets all fields. The hash is calculated and is the only value not provided as an argument.
  - The constructor should be pub(crate) (intended to be called from a factory method on the Ledger type).
  - If business rules are violated, returns an Error with a message detailing the violated rule(s).
- currency
  - Returns the currency of the amount. (Return type unspecified in the draft; behavior is to return amount.currency.)

6. Constraints & Rules (only those explicitly stated or directly implied)
- At least one of sink and source must be not None.
- The amount must always be larger than 0.
- If source is None, then prev_hash_source must be None.
- If source is not None, then prev_hash_source must not be None.
- prev_hash_source must equal the hash of the most recent transaction for the source account (as determined by timestamp).
- prev_hash_sink must equal the hash of the most recent transaction for the sink account (as determined by timestamp), but it may be None even if sink is specified.
- It is not possible for two ledger entries for the same account to have matching timestamps.
- Hash calculation:
  - Computed as a SHA256 over a UTF-8 encoded concatenation of the type’s values, excluding the hash field.
  - Base64 encoding requirements: RFC 4648 §4 alphabet, must include = padding, and must not contain line breaks or whitespace.
  - The exact concatenation and stabilization approach is explicitly an implementation detail.

Inferred Types or Structures (Non-Blocking)
- Location: LedgerEntry (type)
  - Inference made: Type Kind is a Struct.
  - Basis for inference: The draft lists named fields (“Properties”) typical of a record/struct shape.

Implementation Choices Left Open
- Hash input construction (non-blocking): The exact order, delimiters, and formatting used when concatenating field values for hash input, as well as any canonicalization to ensure reproducibility, are explicitly implementation details.
- Visibility mapping (non-blocking): The “pub(crate)” requirement for create is Rust-specific; equivalent restricted visibility in other ecosystems is left to implementation.
- Error detail structure (non-blocking): The exact error type and structure/messages within anyhow::Result are not specified beyond containing information about violated rules.
- Collection/formatting internals (non-blocking): The precise mechanics of Debug formatting used by to_str (e.g., how Option and amount debug output render) are determined by the target platform’s Debug/formatter behavior.