1. Description
A data type representing an amount of a currency. It holds a positive integer value in the currency’s minor unit and the associated currency.

2. Type Kind (Struct / Enum / NewType / Unspecified)
Struct

3. Mutability (Immutable / Mutable)
Immutable

4. Properties (only those explicitly mentioned)
- amount: a positive integer value that denominates the amount in the minor unit of the currency
- currency: uses the currency enum to specify the currency of the amount

5. Functionalities (only those explicitly named)
- major: returns the result of integer division amount / 100
- minor: returns the result of amount modulo 100
- to_str: returns a string formatted as "{major}.{minor} {currency.to_str()}", where minor is zero-padded to two digits when less than 10
- Comparison operators: (<, >, =, >=, <=) are supported between this type and an integer value; the comparison is performed by comparing the amount field (minor-unit value) to that integer

6. Constraints & Rules (only those explicitly stated or directly implied)
- amount must be a positive integer
- major uses integer division by 100
- minor uses modulo 100
- to_str must zero-pad the minor component when it is less than 10 (e.g., “3.05 USD”)
- Comparison semantics are based solely on the amount field (minor-unit numeric value) when compared with an integer

Inferred Types or Structures (Non-Blocking)
- None

Implementation Choices Left Open
- Non-blocking: Exact integer storage widths for amount and for the return values of major/minor outside of Rust. The draft references u64 for major and u16 for minor; in non-Rust implementations, any integer types that can represent these values without loss may be used.
- Non-blocking: Internal collection or representation details (if any) and memory/layout choices.
- Non-blocking: Exact string construction mechanics for to_str as long as the specified format is produced.