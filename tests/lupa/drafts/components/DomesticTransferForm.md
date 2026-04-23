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

- transfer-submitted: fired when the user confirms the transfer via the ConfirmationCard, carries the signed token

## Invariants

- The recipient account number must pass a domestic format check before the submit control enables
- The transfer amount must not exceed the source account's available balance
- The form must obtain a signed token from the server before presenting the ConfirmationCard
- The CSRF token must accompany every state-mutating request

## Scenarios

### user submits a valid domestic transfer
Given: the user has selected source account "Salary", entered recipient 1234-56789012, amount 89,90 EUR, and payment reference "Invoice 2026-04-001"
When: the user submits the form
Then: the form requests server validation, receives a signed token, and displays the ConfirmationCard with the transfer details

### user confirms the transfer via ConfirmationCard
Given: the ConfirmationCard is displayed with a signed token for 89,90 EUR to 1234-56789012
When: the user accepts the ConfirmationCard
Then: transfer-submitted fires carrying the signed token
