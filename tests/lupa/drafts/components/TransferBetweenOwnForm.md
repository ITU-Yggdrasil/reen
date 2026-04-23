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

### user submits a valid own-account transfer
Given: the user has selected source account "Salary" and destination account "Savings" with amount 2 000,00 EUR
When: the user submits the form
Then: the form requests server validation, receives a signed token, and displays the ConfirmationCard

### user confirms the own-account transfer
Given: the ConfirmationCard is displayed with a signed token for 2 000,00 EUR from "Salary" to "Savings"
When: the user accepts the ConfirmationCard
Then: transfer-submitted fires carrying the signed token

### same account selected as source and destination
Given: the user has selected "Salary" as both source and destination
When: the form evaluates the selection
Then: the submit control is disabled with a message explaining that source and destination must differ
