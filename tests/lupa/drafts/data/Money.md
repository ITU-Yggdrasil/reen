# Data: Money

## Purpose
Represents a monetary value in a specific currency. Money is the fundamental unit of financial measurement throughout the banking domain, combining an amount with its denomination so that arithmetic across currencies is never silently mixed. The same shape is used both for account balances and for transaction movements; the sign of the amount carries the distinction (positive for credits and balances, negative for debit movements).

## Attributes
- amount: the quantity in minor units (hundredths of the currency unit), always a whole number; the sign carries semantic meaning (positive for credits and balances, negative for debit movements)
- currency: the ISO 4217 currency code, a three-letter text value (e.g. "DKK", "EUR", "USD")

## Invariants
- Amount may be any whole number, positive, zero, or negative; the type does not enforce a non-negative bound because the same shape is used for transaction movements that can legitimately be negative
- Callers using a Money value to represent an account balance are responsible for ensuring the amount is non-negative; this is a caller-side discipline, not a type-level invariant
- Currency must be a recognised ISO 4217 three-letter code
- Two Money values can only be added or subtracted when their currencies match
- Arithmetic operations must preserve the currency of the operands

## Operations
### add
Combines two Money values with the same currency, producing a new Money whose amount is the sum of the operands. Rejects the operation when currencies differ.

### subtract
Produces a new Money whose amount is the first operand's amount minus the second's. Both must share the same currency. Rejects the operation when currencies differ.

### display as major units
Renders the amount in major currency units with two decimal places (e.g. 10050 minor units in DKK becomes "100.50 DKK").

## Scenarios
### Adding two amounts in the same currency
Given: a Money of 5000 in DKK and a Money of 2500 in DKK
When: add is performed
Then: the result is a Money of 7500 in DKK

### Subtracting two amounts in the same currency
Given: a Money of 10000 in EUR and a Money of 3000 in EUR
When: subtract is performed
Then: the result is a Money of 7000 in EUR

### Adding amounts in different currencies is rejected
Given: a Money of 5000 in DKK and a Money of 2500 in EUR
When: add is performed
Then: the operation is rejected with a currency mismatch error

### Display in major units
Given: a Money of 10050 in DKK
When: display as major units is performed
Then: the result is "100.50 DKK"

### Zero amount is valid
Given: a Money of 0 in USD
When: the Money is created
Then: the Money is accepted with amount 0 and currency "USD"

### Negative amount is valid (debit movement)
Given: a Money of -2500 in DKK representing a debit movement
When: the Money is created
Then: the Money is accepted with amount -2500 and currency "DKK"
