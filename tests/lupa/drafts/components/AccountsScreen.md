# Component: AccountsScreen

## Purpose

The primary post-login landing screen. Displays the user's full portfolio of accounts via an AccountList and provides ambient access to the banking agent through a ChatPane docked to the side or bottom of the viewport.

## Composition

- AccountList: shows all accounts owned by the user
- ChatPane: ambient conversational interface with the banking agent

## Invariants

- The screen requires an authenticated session; unauthenticated visitors must be redirected to LoginScreen
- The AccountList must refresh its data when the screen gains focus after a transfer completion

## Scenarios

### user arrives after login
Given: authenticated user C-9281 navigates to AccountsScreen
When: the screen loads
Then: the AccountList renders with the user's account summaries and the ChatPane is visible

### user selects an account
Given: AccountsScreen is displaying the user's accounts
When: the user taps an account and the AccountList emits account-selected with account identifier FI42 1234 5600 0007 85
Then: the application navigates to AccountDetailScreen for that account
