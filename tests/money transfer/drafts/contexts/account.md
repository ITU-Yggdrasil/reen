# Account

## Props
- **account_id**: the id of the account
- **Ledger**: the main ledger of the system


## Functionality

- **balance**:
The balance of an account is a calculated property. It's the sum of all transactions on the ledger where the account id is the sink minus the sum of all transactions on the ledger where the account id is the source
- **currency**:
All ledger entry for an account must be in the same currency. This is either None or the currency of the latest ledger entry where the account is source or sink
- **account_id**: The id of the account
- **monthly_limit** 100000 minus the amount transferred out of the account within the last 30 days. A transfer is when the account is the source and the sink is settled.
