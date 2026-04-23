# Component: ChatScreen

## Purpose

A dedicated full-screen conversation view for extended interaction with the banking agent. Unlike the ambient ChatPane on AccountsScreen, this screen gives the conversation the full viewport, designed for complex multi-turn flows such as guided transfer setup or account inquiry.

## Composition

- ChatPane: occupies the full content area of the screen

## Invariants

- The screen requires an authenticated session; unauthenticated visitors must be redirected to LoginScreen
- The conversation history must persist across navigation; leaving and returning to ChatScreen must restore the existing session

## Scenarios

### user opens the chat screen
Given: authenticated user C-9281 navigates to ChatScreen with an existing conversation of five messages
When: the screen loads
Then: the ChatPane renders the full five-message thread and the ChatInput is ready for new input

### conversation persists across navigation
Given: the user has sent three messages on the ChatScreen and then navigates to AccountsScreen
When: the user returns to ChatScreen
Then: all three messages and any agent responses are visible in the ChatPane
