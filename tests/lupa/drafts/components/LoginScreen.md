# Component: LoginScreen

## Purpose

The pre-authentication entry point. Presents a branded welcome surface and initiates the Nordea consent redirect flow. Contains no local credential entry; authentication is delegated entirely to the external identity provider.

## Invariants

- The screen must not display any credential input fields; all authentication is handled by the external provider redirect
- If the user is already authenticated, the screen must redirect immediately to AccountsScreen
- After a failed consent flow, the screen must display an error message explaining what happened and offer a retry action

## Scenarios

- Unauthenticated user visits any protected route: the application redirects to LoginScreen
- User activates the login action: the application redirects to the Nordea consent endpoint
- User returns from the consent flow with a valid authorisation: the session establishes and the application navigates to AccountsScreen
- User returns from the consent flow with an error or denial: an error message appears with a retry action
