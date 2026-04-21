# Projection: Account

## Purpose

Account provides a read-only view of a single account's state on the ledger. It is constructed from an account identifier and the ledger, and exposes the account's balance, currency, and transaction history without allowing mutation.

## Source

- Ledger, filtered by account identifier

## Queries

### new
Creates an Account view for a given account identifier and ledger. At least one ledger entry for the account must exist; if none exists, an error is returned. All entries for the account must use the same currency.

### balance
Returns the current balance as an Amount. The balance is the sum of all entries where the account is sink, minus the sum of all entries where the account is source.

### get_currency
Returns the currency of the account, as determined by its ledger entries.

### account_id
Returns the account identifier this view is bound to.

### transactions
Returns all ledger entries related to this account, sorted by timestamp in descending order (newest first).

## Scenarios

### create an account view
Given: a Ledger containing entries for account 123456
When: new is called for account 123456
Then: an Account projection is returned

### reject account with no entries
Given: a Ledger with no entries for account 999
When: new is called for account 999
Then: an error is returned

### compute balance from deposits and withdrawals
Given: account 123456 has a 1000.00 DKK deposit (sink) and a 250.00 DKK withdrawal (source)
When: balance is called
Then: the result is 750.00 DKK

### transactions are newest first
Given: account 123456 has three entries at different timestamps
When: transactions is called
Then: the entries are returned with the most recent first
