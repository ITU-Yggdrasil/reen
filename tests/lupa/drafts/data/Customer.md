# Data: Customer

## Purpose
Represents a banking customer in the system, holding the customer's identity and the set of accounts they own. Customer is the anchor entity that ties together accounts, sessions, and audit trails.

## Attributes
- name: the customer's full legal name, a text value
- customer id: a unique identifier assigned by the bank, a text value (e.g. "CUST-001")
- accounts: a list of references to the Accounts held by this customer

## Invariants
- Customer id must be non-empty and unique across the system
- Name must be non-empty
- The accounts list may be empty (a customer with no accounts yet)

## Scenarios
### Create a customer with accounts
Given: name "Anders Jensen", customer id "CUST-001", accounts referencing "ACC-001" and "ACC-002"
When: a Customer is created
Then: the Customer is accepted with two account references

### Create a customer with no accounts
Given: name "Maria Nielsen", customer id "CUST-002", no account references
When: a Customer is created
Then: the Customer is accepted with an empty accounts list

### Empty customer id is rejected
Given: name "Test Person", customer id "", no account references
When: a Customer is created
Then: the creation is rejected because the customer id is empty

### Empty name is rejected
Given: name "", customer id "CUST-003", no account references
When: a Customer is created
Then: the creation is rejected because the name is empty
