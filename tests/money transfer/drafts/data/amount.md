# Data: Amount

## Purpose

An Amount represents a quantity of money in a specific currency. The value is stored in the currency's minor unit (hundredths), so 10.50 DKK is stored as 1050. Negative values are allowed to represent debits or differences.

## Attributes

- value: the amount in minor units (hundredths of the currency unit)
- currency: the Currency denomination of this amount

## Invariants

- Two Amounts can only be combined (added or subtracted) when their currencies match.

## Operations

### get_amount
Returns the amount as a decimal number by dividing the minor-unit value by 100.

### get_currency
Returns the currency of this amount.

### zero
Given a currency, produces an Amount of zero in that currency.

### addition
Adds two amounts with the same currency. The result is a new Amount with the summed value and the same currency. Panics if the currencies do not match.

### subtraction
Subtracts one amount from another with the same currency. The result is a new Amount with the difference and the same currency. Panics if the currencies do not match.

## Scenarios

### display an amount in minor units
Given: an Amount of 1050 in DKK
When: get_amount is called
Then: the result is 10.50

### add two amounts
Given: an Amount of 500 in DKK and an Amount of 300 in DKK
When: addition is called
Then: the result is an Amount of 800 in DKK

### subtract two amounts
Given: an Amount of 500 in DKK and an Amount of 300 in DKK
When: subtraction is called
Then: the result is an Amount of 200 in DKK

### addition with mismatched currencies panics
Given: an Amount of 500 in DKK and an Amount of 300 in EUR
When: addition is called
Then: the operation panics

### zero amount
Given: the currency DKK
When: zero is called
Then: the result is an Amount of 0 in DKK
