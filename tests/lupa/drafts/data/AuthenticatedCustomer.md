# Data: AuthenticatedCustomer

## Purpose
Represents a role-bindable view of a customer derived from a valid, non-expired session. AuthenticatedCustomer guarantees that any context binding this data can trust that the customer identity has been verified and the session is still active, without re-checking session validity at every interaction.

## Attributes
- customer id: the identifier of the authenticated customer, a text value
- customer name: the full legal name of the customer, a text value
- session reference: a reference to the active Session from which this view was derived
- accounts: a list of references to the Accounts the customer is authorised to access

## Invariants
- The session reference must point to a non-expired session
- The customer id must match the session's customer id
- The accounts list must match the accounts held by the underlying Customer record
- An AuthenticatedCustomer cannot be constructed from an expired session

## Scenarios
### Derive from a valid session
Given: a Session for customer "CUST-001" with expiry "2026-03-15 11:30:00 CET", current time is "2026-03-15 10:45:00 CET", and Customer "CUST-001" named "Anders Jensen" with accounts "ACC-001" and "ACC-002"
When: an AuthenticatedCustomer is derived
Then: the AuthenticatedCustomer is accepted with customer id "CUST-001", customer name "Anders Jensen", and two account references

### Derive from an expired session is rejected
Given: a Session for customer "CUST-001" with expiry "2026-03-15 11:30:00 CET" and current time is "2026-03-15 12:00:00 CET"
When: an AuthenticatedCustomer is derived
Then: the derivation is rejected because the session is expired

### Customer id mismatch is rejected
Given: a Session for customer "CUST-001" and a Customer record with id "CUST-002"
When: an AuthenticatedCustomer is derived
Then: the derivation is rejected because the customer id does not match the session
