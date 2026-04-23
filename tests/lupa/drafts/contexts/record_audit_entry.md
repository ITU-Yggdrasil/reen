# Context: RecordAuditEntry

## Purpose
Appends an immutable audit record for any significant action taken by an authenticated customer. The audit log is append-only and must never be modified or deleted after creation.

## Role Players
- authenticated_customer: AuthenticatedCustomer — the identity on whose behalf the action was performed
- audit_entry: AuditEntry — the immutable record to be assembled and persisted

## Props
- event_kind: the category of action being recorded (e.g. "authentication", "transfer", "account listing")
- description: a human-readable description of what happened, including the outcome
- correlation_id: the request-scoped correlation identifier linking this entry to other records in the same request

## Role Methods
### authenticated_customer
- provide_identity: supplies the customer identifier and session identifier for attribution

### audit_entry
- assemble: combines the customer identity, event kind, description, correlation identifier, and a timestamp into the complete audit record
- persist: writes the assembled audit record to the append-only store, rejecting any attempt to overwrite or delete existing entries

## Functionalities
### record
The authenticated_customer's provide_identity supplies the customer and session identifiers. The audit_entry's assemble combines these with the event kind, description, correlation identifier, and a timestamp to form a complete record. The audit_entry's persist writes the record. The record is immutable once written.

## Scenarios
### successful audit record for account listing
Given: authenticated customer C-9281 with session S-00712, correlation identifier corr-0051, event kind "account listing", description "listed accounts" with outcome "success"
When: record is invoked
Then: an audit entry is assembled and persisted containing customer C-9281, session S-00712, correlation identifier corr-0051, event "listed accounts", outcome "success", and a timestamp

### successful audit record for failed transfer attempt
Given: authenticated customer C-9281 with session S-00712, correlation identifier corr-0052, event kind "transfer", description "transfer between own accounts" with outcome "insufficient funds"
When: record is invoked
Then: an audit entry is assembled and persisted containing customer C-9281, session S-00712, correlation identifier corr-0052, event "transfer between own accounts", outcome "insufficient funds", and a timestamp
