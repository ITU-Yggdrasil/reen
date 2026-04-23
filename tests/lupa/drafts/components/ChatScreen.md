# Component: ChatScreen

## Purpose

A dedicated full-screen conversation view for extended interaction with the banking agent. Unlike the ambient ChatPane on AccountsScreen, this screen gives the conversation the full viewport, suitable for complex multi-turn flows such as guided transfer setup or account inquiry.

## Composition

- ChatPane: occupies the full content area of the screen

## Invariants

- The screen requires an authenticated session; unauthenticated visitors must be redirected to LoginScreen
- The conversation history must persist across navigation; leaving and returning to ChatScreen must restore the existing session

## Scenarios

- User navigates to the chat screen: the existing conversation history loads and the ChatPane renders with the full message thread
- User sends a message: the ChatPane handles submission and the agent responds within the same view
- User navigates away and returns: the conversation resumes from where it was left, with full history intact
