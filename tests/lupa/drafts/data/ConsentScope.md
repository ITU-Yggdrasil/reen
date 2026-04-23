# Data: ConsentScope

## Purpose
Represents a named permission scope that a customer has granted or that the system requires for a specific operation. Consent scopes govern what data and actions are accessible, ensuring the system only performs operations the customer has explicitly authorised under PSD2 and GDPR requirements.

## Attributes
- scope name: the canonical identifier of the permission, a text value — one of:
  - "accounts-read" — permission to view account details and balances
  - "transactions-read" — permission to view transaction history
  - "payments-initiate-own" — permission to initiate own-account transfers
  - "payments-initiate-domestic" — permission to initiate domestic transfers to third parties
  - "payments-initiate-international" — permission to initiate international transfers
  - "customer-info-read" — permission to view customer profile information

## Invariants
- Scope name must be one of the recognised scope identifiers
- Scope names are case-sensitive and must match exactly

## Scenarios
### Valid accounts-read scope
Given: scope name "accounts-read"
When: a ConsentScope is created
Then: the ConsentScope is accepted with scope name "accounts-read"

### Valid payments-initiate-domestic scope
Given: scope name "payments-initiate-domestic"
When: a ConsentScope is created
Then: the ConsentScope is accepted with scope name "payments-initiate-domestic"

### Unrecognised scope name is rejected
Given: scope name "admin-full-access"
When: a ConsentScope is created
Then: the creation is rejected because "admin-full-access" is not a recognised scope

### Case-variant scope name is rejected
Given: scope name "Accounts-Read"
When: a ConsentScope is created
Then: the creation is rejected because scope names are case-sensitive and "Accounts-Read" does not match any recognised scope
