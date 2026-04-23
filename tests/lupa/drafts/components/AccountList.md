# Component: AccountList

## Purpose

Arranges a collection of AccountCards into a vertical list, giving the user an overview of all their accounts. Delegates individual account display to AccountCard and propagates the selection event upward.

## Composition

- AccountCard: one instance per account in the user's portfolio

## Props

- accounts: the ordered collection of account summaries to display

## Events

- account-selected: propagated from the child AccountCard when the user taps an account, carries the account identifier

## Invariants

- Accounts must appear in a stable, deterministic order matching the server-provided sequence
- If the accounts collection is empty, the list must display an informative empty-state message rather than a blank area

## Scenarios

### user taps an account in the list
Given: an AccountList displaying three accounts: "Salary", "Savings", and "Investment"
When: the user taps the "Savings" AccountCard
Then: account-selected fires carrying the identifier of the "Savings" account

### empty account list shows informative message
Given: an AccountList with an empty accounts collection
When: the list renders
Then: an empty-state message explains that no accounts are available
