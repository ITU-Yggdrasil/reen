# Account

## Props
- **account_id**: the id of the account is a positive integer
- **Ledger**: the main ledger of the system


## Functionality

- **balance**:
The balance of an account is a calculated property. It's the sum of all transactions on the ledger where the account id is the sink minus the sum of all transactions on the ledger where the account id is the source. 
- **currency**:
All ledger entries for an account must be in the same currency. This is either None or the currency of previous ledger entries where the account is source or sink, which would all have the same currency due to this constraint.
- **account_id**: The id of the account
- **transactions** returns all ledger entries related to the account, sorted by transactions date, descending
- **new** accepts an account id and the ledger. - at least one entry for the account must exist on the ledger, if not an Error is returned. If at least one ledger entry exist a result of an account object is returned
## Business rules
- **currency** The currency of an account is immutable. Once it is defined (by the first ledger entry), it can't be changed. 

