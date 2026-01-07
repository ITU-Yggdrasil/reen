# Account

## Props
- **account_id**: the id of the account
- **Ledger**: the main ledger of the system


## Functionality

### balance
The balance of an account is a calculated property:
1. It is determined by summing all transactions on the ledger where the `account_id` is the sink.
2. This value is then subtracted from the sum of all transactions on the ledger where the `account_id` is the source.

### currency
- All ledger entries for an account must be in the same currency.
- This currency can either be explicitly specified or determined by the latest ledger entry where the `account_id` is either a source or a sink.

## Unspecified or Ambiguous Aspects

- The process for determining the currency when there are no existing ledger entries.
- Validation rules and error conditions if different currencies are found in ledger entries.
- How the balance calculation handles cases where transactions have not yet been settled.
- The exact nature of what constitutes "settled" in the context of transfers, particularly in relation to the `monthly_limit` calculation.

This specification adheres strictly to the provided draft while surfacing areas that require further clarification.