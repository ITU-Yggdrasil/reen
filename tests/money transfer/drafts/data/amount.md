# amount

## Description

A data type representing an amount of a currency.

## Fields:

- **amount:** an u64 It denominates the amount in the minor unit of the currency
- **currency** : uses the currency enum to specify the currency of the amount

## Functionality

- **major:** returns the amount / 100 (intger division) as an u64
- **minor:** returns the amount modulo 100 as a u16
- **to_str** format: "{major}.{minor} {currency.to_str()}" minor should be zero padded if less that 10

It can also be compared to integers (<,>,=,>=,<=), in which case it's a comparison of the amount to the integer