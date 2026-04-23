# Data: IBAN

## Purpose
Represents an International Bank Account Number, the standardised identifier used across SEPA and international payment networks to uniquely address a bank account.

## Attributes
- value: the full IBAN as a text value, including country code, check digits, and bank-plus-account portion (e.g. "DK5000400440116243")

## Invariants
- Must begin with a two-letter ISO 3166-1 country code
- Must be followed by exactly two check digits
- The remaining characters are alphanumeric and country-specific in length (up to 30 characters for the bank-and-account portion)
- The total length must match the expected length for the given country code
- The check digits must satisfy the MOD-97 validation (ISO 13616)
- Whitespace is stripped on creation but the stored value contains no spaces

## Scenarios
### Valid Danish IBAN
Given: the text "DK5000400440116243"
When: an IBAN is created
Then: the IBAN is accepted with value "DK5000400440116243"

### Valid German IBAN
Given: the text "DE89370400440532013000"
When: an IBAN is created
Then: the IBAN is accepted with value "DE89370400440532013000"

### IBAN with spaces is normalised
Given: the text "DK50 0040 0440 1162 43"
When: an IBAN is created
Then: the IBAN is accepted with value "DK5000400440116243"

### Too-short value is rejected
Given: the text "DK50"
When: an IBAN is created
Then: the creation is rejected because the length is insufficient for country "DK"

### Invalid check digits are rejected
Given: the text "DK0000400440116243"
When: an IBAN is created
Then: the creation is rejected because the check digits do not satisfy MOD-97
