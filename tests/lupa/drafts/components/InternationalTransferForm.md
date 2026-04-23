# Component: InternationalTransferForm

## Purpose

Lets the user send money to a foreign account. Presents a source account selector, an IbanInput for the recipient IBAN, a BIC field, a MoneyInput with cross-currency support, and an optional payment reference. Validates via the server before presenting a ConfirmationCard.

## Composition

- IbanInput: captures and validates the recipient IBAN
- MoneyInput: captures the transfer amount and target currency, with available-currencies reflecting the international transfer corridor
- ConfirmationCard: previews the validated transfer including any indicative exchange rate before final execution
- CsrfFormField: protects the submission against cross-site request forgery

## Props

- accounts: the user's accounts eligible as a source for international transfers
- supported-currencies: the set of currencies available for international transfers

## Events

- transfer-submitted: fired when the user confirms the transfer via the ConfirmationCard, carries the signed token

## Invariants

- The IBAN must pass structural validation via IbanInput before the submit control enables
- The BIC must be syntactically valid before the submit control enables
- The transfer amount must not exceed the source account's available balance, converted to the source currency if the transfer currency differs
- The form must obtain a signed token from the server before presenting the ConfirmationCard
- When the transfer currency differs from the source currency, the ConfirmationCard must display the indicative exchange rate
- The CSRF token must accompany every state-mutating request

## Scenarios

- User fills all fields correctly with a matching currency and submits: the form requests server validation, receives a signed token, and displays the ConfirmationCard
- User fills all fields with a different target currency: the ConfirmationCard additionally shows the indicative exchange rate and the equivalent amount in the source currency
- User enters an invalid IBAN: the IbanInput shows an error and the submit control remains disabled
- User enters an invalid BIC: the BIC field enters an error state and the submit control remains disabled
- User accepts the ConfirmationCard: transfer-submitted fires with the signed token
- User rejects the ConfirmationCard: the form returns to its editable state with previous values intact
