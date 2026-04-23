# Component: MoneyInput

## Purpose

Captures a monetary amount paired with a currency selection. Formats the amount according to the selected currency's conventions and prevents entry of values that exceed the currency's minor-unit precision.

## Props

- amount: the current numeric value, may be empty
- currency: the selected currency code, defaults to the account's native currency
- available-currencies: the set of currencies the user may choose from
- max-amount: optional upper bound, used to prevent transfers exceeding the available balance

## Events

- amount:changed: fired whenever the user modifies the amount, carries the new numeric value
- currency:changed: fired when the user selects a different currency, carries the new currency code

## Invariants

- The entered amount must never have more decimal places than the selected currency allows
- When a max-amount is set, the component must signal an error state if the entered amount exceeds it
- The currency selector must only display currencies present in available-currencies

## Scenarios

- User types an amount within bounds: the value updates and amount:changed fires with the formatted value
- User types more decimal places than the currency permits: input is truncated to the allowed precision
- User enters an amount exceeding max-amount: the field enters an error state with an explanatory message
- User switches currency: currency:changed fires and the decimal precision rule re-evaluates against the new currency
