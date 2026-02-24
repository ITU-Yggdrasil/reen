# amount

## Description

A data type representing an amount of a currency.

## Fields:

- **amount** an integer value that denominates the amount in the minor unit of the currency. Negative values are allowed
- **currency** : uses the currency enum to specify the currency of the amount

## Functionality

- **major** major equals abs(amount) / 100 with the sign applied separately
- **minor** returns the absolute value iof amount modulo 100.
- **to_str** format: "{major}.{minor} {currency.to_str()}" minor should be zero padded if less that 10
- **get_currency** returns the currency

**comparisons** two amount objects can be comapred with <,>,=, >= and <= which has the same result as if the amount field of the twon objects was compared.ß
**addition** two amount objects can be added provided that the currency is the same for both. THe result is a new account object where the amount is set to the sum of the amounts of the operands. The currency is unchanged
**subtraction** two amount objects can be subtracted provided that the currency is the same for both. the result is a new amount object where the amount is set to the difference of the amounts of the operands. The currency remains unchanged.