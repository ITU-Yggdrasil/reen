# Component: ConfirmationCard

## Purpose

Presents a read-only preview of a pending transfer and asks the user to accept or reject it. Carries a signed token that binds the displayed details to the server-validated transfer intent, preventing tampering between preview and execution.

## Props

- from-account: the source account label and masked number
- to-account: the destination account label or IBAN
- amount: the transfer amount and currency
- description: the optional payment reference or memo
- token: a signed opaque token representing the server-validated transfer intent

## Events

- accepted: fired when the user confirms the transfer, carries the signed token
- rejected: fired when the user declines the transfer

## Invariants

- The card must be read-only; no displayed value may be editable
- The accept action must transmit the original token without modification
- If the token is absent or empty, the accept control must be disabled

## Scenarios

### user accepts the transfer
Given: a ConfirmationCard showing a transfer of 500,00 EUR from "Salary (**** 0785)" to "Savings (**** 0786)" with token tok-ABC123
When: the user taps the accept control
Then: accepted fires carrying token tok-ABC123 and the card enters a busy state

### user rejects the transfer
Given: a ConfirmationCard showing a pending transfer
When: the user taps the reject control
Then: rejected fires and the user returns to the form
