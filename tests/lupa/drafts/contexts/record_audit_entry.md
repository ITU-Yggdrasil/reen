# Context: RecordAuditEntry

## Purpose
Appends an immutable audit record for any significant action taken by an authenticated customer. The audit log is append-only and must never be modified or deleted after creation.

## Role Players
- authenticated_customer: AuthenticatedCustomer — the identity on whose behalf the action was performed
- event_descriptor: a description of what happened (the action name, target resource, and outcome)
- correlation_id: the request-scoped correlation identifier linking this entry to other records in the same request
- audit_sink: AuditSink — the append-only store that receives the entry

## Role Methods
### authenticated_customer
- provide_identity: supplies the customer identifier and session identifier for attribution

### audit_sink
- append: writes the assembled audit record to the append-only store, rejecting any attempt to overwrite or delete existing entries

## Functionalities
### record
The authenticated_customer's provide_identity supplies the customer and session identifiers. These are combined with the event_descriptor, the correlation_id, and a timestamp to form an audit record. The audit_sink's append writes the record. The record is immutable once written.

## Scenarios
### successful audit record for account listing
Given: authenticated customer C-9281 with session S-00712 and correlation identifier corr-0051
When: record is invoked with event descriptor "listed accounts" and outcome "success"
Then: an audit entry is appended containing customer C-9281, session S-00712, correlation identifier corr-0051, event "listed accounts", outcome "success", and a timestamp

### successful audit record for failed transfer attempt
Given: authenticated customer C-9281 with session S-00712 and correlation identifier corr-0052
When: record is invoked with event descriptor "transfer between own accounts" and outcome "insufficient funds"
Then: an audit entry is appended containing customer C-9281, session S-00712, correlation identifier corr-0052, event "transfer between own accounts", outcome "insufficient funds", and a timestamp
