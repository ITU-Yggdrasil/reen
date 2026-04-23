# Component: DomesticTransferForm

## Purpose

Lets the user send money to another account within the same country. Presents a source account selector, a recipient account number field, a MoneyInput for the amount, and an optional payment reference. Validates via the server before presenting a ConfirmationCard.

## Composition

- MoneyInput: captures the transfer amount and currency
- ConfirmationCard: previews the validated transfer before final execution
- CsrfFormField: protects the submission against cross-site request forgery

## Props

- accounts: the user's accounts eligible as a source for domestic transfers

## Events

- transfer:submitted: fired when the user confirms the transfer via the ConfirmationCard, carries the signed token

## Invariants

- The recipient account number must pass a domestic format check before the submit control enables
- The transfer amount must not exceed the source account's available balance
- The form must obtain a signed token from the server before presenting the ConfirmationCard
- The CSRF token must accompany every state-mutating request

## Scenarios

- User fills all fields correctly and submits: the form requests server validation, receives a signed token, and displays the ConfirmationCard
- User enters an invalid recipient account number: the field enters an error state and the submit control remains disabled
- User attempts to transfer more than the available balance: the MoneyInput enters an error state
- User accepts the ConfirmationCard: transfer:submitted fires with the signed token
- User rejects the ConfirmationCard: the form returns to its editable state with previous values intact
