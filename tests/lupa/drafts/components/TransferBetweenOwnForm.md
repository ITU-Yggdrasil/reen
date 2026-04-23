# Component: TransferBetweenOwnForm

## Purpose

Lets the user move money between their own accounts. Presents source and destination account selectors, a MoneyInput for the amount, and a submit control that requests server-side validation before showing a ConfirmationCard.

## Composition

- MoneyInput: captures the transfer amount and currency
- ConfirmationCard: previews the validated transfer before final execution
- CsrfFormField: protects the submission against cross-site request forgery

## Props

- accounts: the user's accounts eligible for internal transfers

## Events

- transfer-submitted: fired when the user confirms the transfer via the ConfirmationCard, carries the signed token

## Invariants

- The source and destination accounts must differ; selecting the same account for both must disable the submit control
- The transfer amount must not exceed the source account's available balance
- The form must obtain a signed token from the server before presenting the ConfirmationCard
- The CSRF token must accompany every state-mutating request

## Scenarios

- User selects source, destination, and a valid amount then submits: the form requests server validation, receives a signed token, and displays the ConfirmationCard
- User attempts to transfer more than the available balance: the MoneyInput enters an error state and the submit control remains disabled
- User selects the same account as both source and destination: the submit control disables with an explanatory message
- User accepts the ConfirmationCard: transfer-submitted fires with the signed token
- User rejects the ConfirmationCard: the form returns to its editable state with previous values intact
