# Data: IBAN

## Purpose
Represents an International Bank Account Number, the standardised identifier used across SEPA and international payment networks to uniquely address a bank account.

## Attributes
- value: the full IBAN as a text value, including country code, check digits, and bank-plus-account portion (e.g. "DK5000400440116243")

## Invariants
- Must begin with a two-letter country code that is a recognised ISO 3166-1 alpha-2 entry; the country code is validated against the ISO 3166-1 registry, not just against the two-letter syntactic shape
- Must be followed by exactly two check digits
- The remaining characters are alphanumeric and country-specific in length (up to 30 characters for the bank-and-account portion)
- The total length must match the expected length for the given country code, per the country length table below
- The check digits must satisfy the MOD-97 validation (ISO 13616)
- Whitespace is stripped on creation but the stored value contains no spaces

## Country Length Table
The total IBAN length (including country code and check digits) is fixed per country. The recognised lengths are:

- AD (Andorra): 24
- AT (Austria): 20
- BE (Belgium): 16
- BG (Bulgaria): 22
- CH (Switzerland): 21
- CY (Cyprus): 28
- CZ (Czech Republic): 24
- DE (Germany): 22
- DK (Denmark): 18
- EE (Estonia): 20
- ES (Spain): 24
- FI (Finland): 18
- FO (Faroe Islands): 18
- FR (France): 27
- GB (United Kingdom): 22
- GL (Greenland): 18
- GR (Greece): 27
- HR (Croatia): 21
- HU (Hungary): 28
- IE (Ireland): 22
- IS (Iceland): 26
- IT (Italy): 27
- LI (Liechtenstein): 21
- LT (Lithuania): 20
- LU (Luxembourg): 20
- LV (Latvia): 21
- MC (Monaco): 27
- MT (Malta): 31
- NL (Netherlands): 18
- NO (Norway): 15
- PL (Poland): 28
- PT (Portugal): 25
- RO (Romania): 24
- SE (Sweden): 24
- SI (Slovenia): 19
- SK (Slovakia): 24
- SM (San Marino): 27
- VA (Vatican City): 22

A country code outside this table is rejected as unsupported.

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

### Unrecognised country code is rejected
Given: the text "ZZ5000400440116243"
When: an IBAN is created
Then: the creation is rejected because "ZZ" is not a recognised ISO 3166-1 alpha-2 country code

### Country-length mismatch is rejected
Given: the text "DK500040044011624399" (twenty characters, but DK requires eighteen)
When: an IBAN is created
Then: the creation is rejected because the length does not match the expected length of 18 for country "DK"

### Norwegian IBAN matches expected length of 15
Given: the text "NO9386011117947"
When: an IBAN is created
Then: the IBAN is accepted with value "NO9386011117947"
