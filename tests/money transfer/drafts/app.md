# The primary application

## Application Kind
`cli_app`

## Purpose

A simple test application that transfers money between two accounts using an immutable ledger.

## Collaborators

- MoneyTransfer: enacts the transfer of funds from source to sink
- Ledger: stores all transaction entries
- Account: provides a read-only view of an account's state on the ledger

## Startup Sequence

Create initial ledger entries for two accounts:
- Account 123456 receives an initial deposit of 1000.00 DKK (source absent, sink is 123456)
- Account 654321 receives an initial deposit of 1000.00 DKK (source absent, sink is 654321)

## Main Flow

1. Transfer 250.00 DKK from account 123456 to account 654321 using MoneyTransfer.
2. After the transfer completes, print the transaction history for each account to standard output. Each transaction is a ledger entry.
3. Exit with code 0.

## Error Handling

If a runtime error occurs, exit with code 42. If an error message is available, print it to standard error.
