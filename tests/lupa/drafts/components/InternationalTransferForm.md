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

### user submits a valid international transfer
Given: the user has selected source account "Salary", entered IBAN DE89 3704 0044 0532 0130 00, BIC COBADEFFXXX, amount 3 500,00 EUR, and payment reference "Contract DE-2026-118"
When: the user submits the form
Then: the form requests server validation, receives a signed token, and displays the ConfirmationCard with the transfer details

### user confirms the international transfer
Given: the ConfirmationCard is displayed with a signed token for 3 500,00 EUR to DE89 3704 0044 0532 0130 00
When: the user accepts the ConfirmationCard
Then: transfer-submitted fires carrying the signed token
