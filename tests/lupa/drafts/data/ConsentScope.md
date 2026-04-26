# Data: ConsentScope

## Purpose
Represents a named permission scope that a customer has granted or that the system requires for a specific operation. Consent scopes govern what data and actions are accessible, ensuring the system only performs operations the customer has explicitly authorised under PSD2 and GDPR requirements.

## Attributes
- scope name: the canonical identifier of the permission, a text value drawn from a fixed enumeration. The recognised scope names are "accounts-read" (view account details and balances), "transactions-read" (view transaction history), "payments-initiate-own" (initiate own-account transfers), "payments-initiate-domestic" (initiate domestic transfers to third parties), "payments-initiate-international" (initiate international transfers), and "customer-info-read" (view customer profile information).

## Invariants
- Scope name must be exactly one of the recognised scope identifiers: "accounts-read", "transactions-read", "payments-initiate-own", "payments-initiate-domestic", "payments-initiate-international", or "customer-info-read"
- Scope names are case-sensitive and must match exactly; "Accounts-Read" and "ACCOUNTS-READ" are not the same as "accounts-read"

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
