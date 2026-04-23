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

- User has three accounts: three AccountCards render in order, each showing its summary
- User taps the second account: account-selected fires with that account's identifier
- User has no accounts: an empty-state message explains that no accounts are available
