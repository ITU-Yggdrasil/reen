# Data: BIC

## Purpose
Represents a Bank Identifier Code (also known as SWIFT code), used to identify a specific financial institution in international wire transfers and SEPA payments.

## Attributes
- value: the BIC as a text value, either eight or eleven characters (e.g. "NDEADKKK" or "NDEADKKKXXX")

## Invariants
- Must be exactly eight or eleven alphanumeric characters
- The first four characters are the bank code (letters only)
- Characters five and six are the ISO 3166-1 country code (letters only)
- Characters seven and eight are the location code (letters or digits)
- If eleven characters, the last three are the branch code (letters or digits)
- An eight-character BIC implicitly represents the primary office

## Scenarios
### Valid eight-character BIC
Given: the text "NDEADKKK"
When: a BIC is created
Then: the BIC is accepted with value "NDEADKKK"

### Valid eleven-character BIC
Given: the text "NDEADKKKXXX"
When: a BIC is created
Then: the BIC is accepted with value "NDEADKKKXXX"

### Too-short value is rejected
Given: the text "NDEA"
When: a BIC is created
Then: the creation is rejected because the length is neither eight nor eleven

### Non-alphabetic bank code is rejected
Given: the text "1234DKKK"
When: a BIC is created
Then: the creation is rejected because the bank code portion contains non-letter characters

### Ten-character value is rejected
Given: the text "NDEADKKKXX"
When: a BIC is created
Then: the creation is rejected because the length is neither eight nor eleven
