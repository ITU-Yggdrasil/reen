# Data: Account

## Purpose
Represents a bank account held at Nordea, containing the identifying information, currency denomination, current balance, and ownership reference needed to participate in payment and balance-enquiry scenarios.

## Attributes
- alias: a short human-chosen label for the account, a text value (e.g. "Household"), present or absent
- name: the official account name as registered with the bank, a text value
- kind: the account classification — one of "current", "savings", or "business"
- currency: the ISO 4217 currency code for this account, a three-letter text value
- balance: the current available balance, a Money value
- iban: the international account identifier, an IBAN value
- account number: the domestic account identifier, a DomesticAccountNumber value
- owning customer: a reference to the Customer who holds this account

## Invariants
- The balance's currency must always match the account's currency
- The IBAN and account number must both refer to the same underlying account
- Kind must be one of the recognised classifications
- Every account must have an owning customer reference

## Operations
### credit
Increases the balance by a given Money amount. The credited Money must share the account's currency.

### debit
Decreases the balance by a given Money amount. The debited Money must share the account's currency. The balance must not fall below zero for savings accounts; current and business accounts may have an overdraft policy but that is outside this data's scope.

## Scenarios
### Create a current account
Given: name "Loenmodtagerkonto", kind "current", currency "DKK", balance of 150000 in DKK, IBAN "DK5000400440116243", account number "0040-0440116243", owning customer "CUST-001"
When: an Account is created
Then: the Account is accepted with all attributes set

### Credit an account
Given: an Account with balance of 150000 in DKK
When: credit is performed with a Money of 25000 in DKK
Then: the balance becomes 175000 in DKK

### Debit an account
Given: an Account with balance of 150000 in DKK
When: debit is performed with a Money of 30000 in DKK
Then: the balance becomes 120000 in DKK

### Currency mismatch on credit is rejected
Given: an Account with currency "DKK" and balance of 150000 in DKK
When: credit is performed with a Money of 5000 in EUR
Then: the operation is rejected with a currency mismatch error

### Alias is absent
Given: an Account created without an alias
When: the alias is inspected
Then: the alias is absent

### Account with alias
Given: an Account created with alias "Household"
When: the alias is inspected
Then: the alias is present with value "Household"
