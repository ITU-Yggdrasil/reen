# amount

## Description

A data type representing an amount of a currency.

## Fields:

- **amount** an integer value that denominates the amount in the minor unit of the currency. Negative values are allowed
- **currency** : uses the currency enum to specify the currency of the amount

## Functionality

- **get_amount** amount / 100 (floating point division)
- **get_currency** returns the currency
- **zero** takes a currency and returns an amount object with the amount set to zero
**addition** two amount objects can be added provided that the currency is the same for both. The result is a new amount object where the amount is set to the sum of the amounts of the operands. The currency is unchanged. If the currencies don't match we panic
**subtraction** two amount objects can be subtracted provided that the currency is the same for both. the result is a new amount object where the amount is set to the difference of the amounts of the operands. The currency remains unchanged. If the currencies don't match we panic
