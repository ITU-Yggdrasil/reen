# Data: DomesticAccountNumber

## Purpose
Represents a Danish domestic bank account number in the traditional registration-number-plus-account-number format, used for domestic transfers within the Danish banking system.

## Attributes
- registration number: a four-digit text value identifying the bank branch (e.g. "0040")
- account number: a text value of up to ten digits identifying the account within that branch (e.g. "0440116243")

## Invariants
- Registration number must be exactly four digits
- Account number must be between one and ten digits
- Both parts contain only digits, no letters or special characters
- Leading zeroes are preserved in both parts

## Operations
### format as display string
Produces the conventional Danish display format with the registration number and account number separated by a hyphen (e.g. "0040-0440116243").

## Scenarios
### Valid domestic account number
Given: registration number "0040" and account number "0440116243"
When: a DomesticAccountNumber is created
Then: the DomesticAccountNumber is accepted

### Display format
Given: a DomesticAccountNumber with registration number "0040" and account number "0440116243"
When: format as display string is performed
Then: the result is "0040-0440116243"

### Registration number too short is rejected
Given: registration number "040" and account number "0440116243"
When: a DomesticAccountNumber is created
Then: the creation is rejected because the registration number is not exactly four digits

### Account number too long is rejected
Given: registration number "0040" and account number "04401162430000"
When: a DomesticAccountNumber is created
Then: the creation is rejected because the account number exceeds ten digits

### Non-digit characters in account number are rejected
Given: registration number "0040" and account number "044ABC6243"
When: a DomesticAccountNumber is created
Then: the creation is rejected because the account number contains non-digit characters
