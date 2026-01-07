## Ledger Entry

### Description
A ledger entry is an entry in the main ledger. It has a source account, a destination account, a nominal amount and a currency.

The source might be `None` signifying that it's a cash deposit, the sink would on the other hand be `None` if it's a cash withdrawal.

If a transfer is reflected by the ledger entry, then both sink and source will be `Some(...)`. The nominal amount must be greater than 0.

### Type Kind
Struct

### Mutability
Immutable

### Properties
- **sink:** Option<integer>
- **source:** Option<integer>
- **amount:** Nominal amount (must be larger than zero)
- **currency:** The currency of the transfer.

### Functionalities
- None explicitly named in the draft.

### Constraints & Rules
- Both `sink` and `source` cannot both be `None` at the same time.
- If a transfer is being represented, then both `sink` and `source` must be present (i.e., not `None`).
- The nominal amount (`amount`) must be greater than zero.

### Unspecified or Ambiguous Aspects
- What exactly does `Some(...)` represent in terms of account numbers or identifiers?
- How is the currency represented or formatted?
- No explicit rules about validation, handling of negative amounts, or other edge cases.