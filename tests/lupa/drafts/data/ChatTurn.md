# Data: ChatTurn

## Purpose
Represents a single exchange unit in a chat conversation. ChatTurn is a variant type whose kind determines the content: a user's message, the agent's response, a tool invocation, a tool result, or a confirmation card lifecycle event (rendered, accepted, or rejected).

## Attributes
- kind: the variant classification — one of "user-utterance", "agent-response", "tool-call", "tool-result", "confirmation-card-rendered", "confirmation-card-accepted", "confirmation-card-rejected"
- timestamp: the moment this turn occurred, a date-and-time value with timezone

### User-utterance attributes
- text: the customer's message, a text value

### Agent-response attributes
- text: the agent's reply, a text value

### Tool-call attributes
- tool name: the name of the tool being invoked, a text value
- tool input: the input provided to the tool, a text value describing the parameters

### Tool-result attributes
- tool name: the name of the tool that produced this result, a text value
- tool output: the result returned by the tool, a text value

### Confirmation-card-rendered attributes
- card reference: a reference to the ConfirmationCard that was shown to the customer

### Confirmation-card-accepted attributes
- card reference: a reference to the ConfirmationCard the customer accepted

### Confirmation-card-rejected attributes
- card reference: a reference to the ConfirmationCard the customer rejected
- reason: the customer's stated reason for rejection, a text value (present or absent)

## Invariants
- Kind must be one of the recognised variant classifications
- Timestamp must not be in the future
- Each variant must carry exactly the attributes defined for its kind
- User-utterance text must be non-empty
- Agent-response text must be non-empty
- Tool-call tool name must be non-empty

## Scenarios
### User utterance
Given: kind "user-utterance", timestamp "2026-03-15 10:31:00 CET", text "Transfer 250 DKK to my savings account"
When: a ChatTurn is created
Then: the ChatTurn is accepted as a user utterance

### Agent response
Given: kind "agent-response", timestamp "2026-03-15 10:31:02 CET", text "I'll prepare that transfer for you. Please confirm the details."
When: a ChatTurn is created
Then: the ChatTurn is accepted as an agent response

### Tool call
Given: kind "tool-call", timestamp "2026-03-15 10:31:01 CET", tool name "get-accounts", tool input "customer id CUST-001"
When: a ChatTurn is created
Then: the ChatTurn is accepted as a tool call

### Tool result
Given: kind "tool-result", timestamp "2026-03-15 10:31:01 CET", tool name "get-accounts", tool output "2 accounts found: Loenmodtagerkonto (DKK), Savings (DKK)"
When: a ChatTurn is created
Then: the ChatTurn is accepted as a tool result

### Confirmation card accepted
Given: kind "confirmation-card-accepted", timestamp "2026-03-15 10:31:30 CET", card reference "CARD-001"
When: a ChatTurn is created
Then: the ChatTurn is accepted as a confirmation card acceptance

### Confirmation card rejected with reason
Given: kind "confirmation-card-rejected", timestamp "2026-03-15 10:31:30 CET", card reference "CARD-001", reason "Wrong amount"
When: a ChatTurn is created
Then: the ChatTurn is accepted as a confirmation card rejection with reason "Wrong amount"

### Empty user utterance is rejected
Given: kind "user-utterance", timestamp "2026-03-15 10:31:00 CET", text ""
When: a ChatTurn is created
Then: the creation is rejected because the user utterance text is empty
