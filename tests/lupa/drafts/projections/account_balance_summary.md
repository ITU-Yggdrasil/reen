# Projection: AccountBalanceSummary

## Purpose

Provides a one-line display summary of an account's name and current balance, formatted for UI rendering.

## Role Players

- account: Account — provides the display name and current balance to summarise

## Role Methods

### account
- read_display_name: provides the account's human-readable display name
- read_balance: provides the current balance with its currency

## Functionalities

### summary
Returns a single text line combining the account's display name and its current balance expressed in the account's currency. The balance uses the brand's numeric typography role for consistent formatting.

## Scenarios

### salary account with positive balance
Given: an account named "Salary" with balance of five hundred Danish kroner
When: summary is queried
Then: returns "Salary — 500,00 DKK"

### savings account with zero balance
Given: an account named "Savings" with zero balance in euros
When: summary is queried
Then: returns "Savings — 0,00 EUR"
