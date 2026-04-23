# Component: AccountDetailScreen

## Purpose

Shows the full transaction history for a single account. Displays the account summary at the top and a TransactionTable below, allowing the user to filter transactions by date range.

## Composition

- AccountCard: displays the selected account's summary at the top of the screen
- TransactionTable: lists and filters transactions for the account

## Props

- account-id: the identifier of the account to display, received from the route parameter

## Invariants

- The screen requires an authenticated session; unauthenticated visitors must be redirected to LoginScreen
- If the account-id does not correspond to an account owned by the current user, the screen must show an error and offer navigation back to AccountsScreen

## Scenarios

- User navigates to an account detail: the screen fetches the account summary and recent transactions, rendering the AccountCard and TransactionTable
- User adjusts the date range: TransactionTable emits filter-changed and the screen fetches the filtered transaction set
- User navigates to a non-existent or unauthorised account: an error message appears with a link back to AccountsScreen
