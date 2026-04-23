# Component: MoneyInput

## Purpose

Captures a monetary amount paired with a currency selection. Formats the amount according to the selected currency's conventions and prevents entry of values that exceed the currency's minor-unit precision.

## Props

- amount: the current numeric value, may be empty
- currency: the selected currency code, defaults to the account's native currency
- available-currencies: the set of currencies the user may choose from
- max-amount: optional upper bound, used to prevent transfers exceeding the available balance

## Events

- amount-changed: fired whenever the user modifies the amount, carries the new numeric value
- currency-changed: fired when the user selects a different currency, carries the new currency code

## Invariants

- The entered amount must never have more decimal places than the selected currency allows
- When a max-amount is set, the component must signal an error state if the entered amount exceeds it
- The currency selector must only display currencies present in available-currencies

## Scenarios

### user enters a valid amount
Given: a MoneyInput with currency EUR and max-amount 10 000,00
When: the user types 250,50
Then: amount-changed fires with the value 250,50

### user switches currency
Given: a MoneyInput with currency EUR and available currencies EUR and GBP
When: the user selects GBP
Then: currency-changed fires with GBP and the decimal precision adjusts to GBP rules

### amount exceeds maximum
Given: a MoneyInput with currency EUR and max-amount 1 000,00
When: the user types 1 500,00
Then: the field enters an error state with a message indicating the maximum is exceeded
