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

### user opens account detail screen
Given: authenticated user navigates to account detail for account FI42 1234 5600 0007 85
When: the screen loads
Then: the AccountCard displays the account summary and the TransactionTable shows the most recent transactions

### user filters transactions by date range
Given: the AccountDetailScreen is displaying account FI42 1234 5600 0007 85
When: the user changes the date range in the TransactionTable's DateRangePicker to 2026-04-01 through 2026-04-15
Then: the screen fetches transactions for the new range and the TransactionTable re-renders with the filtered set

### unauthorised account shows error
Given: authenticated user C-9281 navigates to account detail for account FI42 9999 0000 0001 00 which belongs to customer C-6633
When: the screen loads
Then: an error message appears and a link navigates back to AccountsScreen
