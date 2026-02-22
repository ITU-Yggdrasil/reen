1. Description
A data type representing a monetary amount in the minor unit of a specific currency.

2. Type Kind (Struct / Enum / NewType / Unspecified)
Struct

3. Mutability (Immutable / Mutable)
Immutable

4. Properties (only those explicitly mentioned)
- amount: a positive integer value that denominates the amount in the minor unit of the currency (exact integer type unspecified)
- currency: currency (the enum defined in the direct dependency “currency”)

5. Functionalities (only those explicitly named)
- major: returns amount divided by 100 using integer division; return type u64
- minor: returns amount modulo 100; return type u16
- to_str: returns a string formatted as "{major}.{minor} {currency.to_str()}", where:
  - major is the result of major()
  - minor is the result of minor(), rendered zero-padded to two digits when its value is less than 10
  - currency.to_str() is the three-letter code returned by the currency dependency’s to_str
- get_currency: returns the currency property; return type currency

6. Constraints & Rules (only those explicitly stated or directly implied)
- amount must be a strictly positive integer
- amount represents the value in the minor unit of the specified currency
- major uses integer division by 100 on amount
- minor uses modulo 100 on amount
- In to_str, minor must be zero-padded to two digits when its numeric value is less than 10

Inferred Types or Structures (Non-Blocking)
- Function: to_str
  - Inference: returns a string value
  - Basis: The draft specifies a concrete output “format: '{major}.{minor} {currency.to_str()}'”, which implies a string result.

Implementation Choices Left Open
- The concrete integer type and width used to store amount (e.g., specific bit width) is not specified and can be chosen by the implementation, provided it supports the defined operations and constraints.
- Exact formatting mechanics to achieve zero-padding (e.g., specific formatting library or function) are not specified, as long as the output format requirement is met.