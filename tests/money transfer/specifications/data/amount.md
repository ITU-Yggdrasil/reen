1. Description
A data type representing an amount of a currency. The numeric value is stored in the minor unit of the currency.

2. Type Kind (Struct / Enum / NewType / Unspecified)
Struct

3. Mutability (Immutable / Mutable)
Immutable

4. Properties (only those explicitly mentioned)
- amount: u64
  - Denominates the amount in the minor unit of the currency.
- currency: currency
  - Uses the currency enum to specify the currency of the amount.

5. Functionalities (only those explicitly named)
- major:
  - Returns amount / 100 using integer division as a u64.
- minor:
  - Returns amount modulo 100 as a u16.
- to_str:
  - Returns a string formatted as: "{major}.{minor} {currency.to_str()}".
  - The minor part is zero-padded when its value is less than 10.
- Comparisons with integers:
  - Operators: <, >, =, >=, <=
  - Semantics: compares the amount field to the provided integer value.

6. Constraints & Rules (only those explicitly stated or directly implied)
- major uses integer division (truncating) by 100 on the amount field.
- minor is the remainder of amount modulo 100 and therefore in the range 0..=99.
- to_str must:
  - Use currency.to_str() for the currency code.
  - Zero-pad the minor component to exactly two digits when its value is 0..9.
- Comparisons with integers consider only the amount field; the currency field is not involved.

Inferred Types or Structures (Non-Blocking)
- Location: Comparisons with integers
  - Inference made: The unspecified “integer” type is treated as i32.
  - Basis for inference: Conventional Rust default for unspecified integer types.
- Location: to_str
  - Inference made: The formatted value is produced via Rust formatting (format!) and thus yields a string value.
  - Basis for inference: Allowed convention for formatting and string production.

Unspecified or Ambiguous Aspects
- Scope of supported integer types for comparisons beyond the inferred default (i32) is unspecified.
- Behavior when comparing against negative integers (if supported by the chosen integer type) is unspecified.
- Serialization/deserialization behavior for amount is unspecified (no guidance provided in the draft for this type).

Worth to Consider
- Non-blocking, out-of-scope: Some currencies use minor unit exponents other than 2; the fixed division/modulo by 100 may not align with all ISO 4217 currencies.
- Non-blocking, out-of-scope: Locale-aware or thousand-separator formatting for to_str is not addressed.
- Non-blocking, out-of-scope: Deriving standard traits (e.g., serde Serialize/Deserialize, Eq/Ord, Display/FromStr) for integration and ergonomics.