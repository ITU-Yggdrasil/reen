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

- User reviews a correct transfer and taps accept: transfer-accepted fires with the token and the card enters a busy state
- User notices an error and taps reject: transfer-rejected fires and the user returns to the form with their previous input preserved
- Token is missing due to a server error: the accept control is disabled and an explanatory message appears
