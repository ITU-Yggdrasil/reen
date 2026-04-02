# amount

## Description

A data type representing an amount of a currency.

## Fields

| Field | Meaning | Notes |
|---|---|---|
| amount | Integer value in the currency's minor unit | Negative values are allowed |
| currency | Currency enum for the amount | Uses the Currency type |

## Functionalities

- **get_amount** Returns `amount / 100` using floating-point division.
- **get_currency** Returns the currency.
- **zero** Takes a currency and returns an amount object with amount set to zero.
- **addition** Two amount objects can be added when their currencies match. The result is a new amount object with the summed amount and the same currency. If the currencies do not match, the operation panics.
- **subtraction** Two amount objects can be subtracted when their currencies match. The result is a new amount object with the difference and the same currency. If the currencies do not match, the operation panics.
