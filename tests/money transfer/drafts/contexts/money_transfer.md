# Context: MoneyTransfer

## Purpose

MoneyTransfer enacts the transfer of a specific amount from one account to another. It coordinates the source and sink accounts through the ledger, ensuring that the source has sufficient funds and that the resulting ledger entries correctly represent the movement of money.

## Role Players

- source: Account — provides the funds; must have sufficient balance to cover the transfer
- sink: Account — receives the funds

## Props

- amount: the Amount to transfer between the accounts
- ledger: the Ledger that records all transactions

## Role Methods

### source
- withdraw: creates an unsettled ledger entry representing a withdrawal of the transfer amount from the source account; the transfer amount must not exceed the source's current balance; returns an error if business rules are violated

### sink
- deposit: settles the unsettled entry created by withdraw, recording the sink as the destination; the resulting entry represents a complete transfer from source to sink

## Functionalities

### new
Constructs the money transfer context. Receives the account identifiers for source and sink, the amount, and the ledger. Constructs Account views for both source and sink. The currency of the amount must match the currency of the source account. Returns an error if account construction fails or if the currency does not match.

### transfer
Executes the transfer by calling the source's withdraw role method, then the sink's deposit role method, then committing the resulting entry to the ledger. If any step fails, the operation returns an error immediately. On success, returns the updated ledger.

## Scenarios

### successful transfer
Given: account 123456 has balance 1000.00 DKK, account 654321 has balance 1000.00 DKK, amount is 250.00 DKK
When: transfer is called
Then: the returned ledger shows account 123456 with balance 750.00 DKK and account 654321 with balance 1250.00 DKK

### insufficient funds
Given: account 123456 has balance 100.00 DKK, amount is 250.00 DKK
When: transfer is called
Then: an error is returned indicating insufficient funds

### currency mismatch on construction
Given: account 123456 uses DKK, amount is 250.00 EUR
When: new is called
Then: an error is returned indicating the currency does not match
