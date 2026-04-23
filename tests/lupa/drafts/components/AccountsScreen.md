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

- User arrives after login: the screen fetches account summaries and renders the AccountList alongside the ambient ChatPane
- User taps an account: AccountList emits account:selected and the application navigates to AccountDetailScreen for that account
- User sends a message in the ChatPane: the message is delivered to the ChatSession context and the agent responds within the pane
