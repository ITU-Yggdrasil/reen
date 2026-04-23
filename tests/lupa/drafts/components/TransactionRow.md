# Component: TransactionRow

## Purpose

Represents a single transaction in a list, showing the date, counterparty name, and signed amount. Positive amounts appear in positive-delta colour and negative amounts in negative-delta colour.

## Props

- date: the booking date of the transaction
- counterparty: the name of the other party
- amount: the signed monetary value, positive for credits and negative for debits
- currency: the currency of the transaction

## Invariants

- Positive amounts must display in positive-delta and carry a leading plus sign
- Negative amounts must display in negative-delta and carry a leading minus sign
- All amounts use mono-numeric typography and show exactly two decimal places for fiat currencies

## Scenarios

- User views a credit transaction: the row shows the date, counterparty, and a green-toned positive amount
- User views a debit transaction: the row shows the date, counterparty, and a red-toned negative amount
- Multiple rows appear together: amounts align in a column thanks to mono-numeric tabular figures
