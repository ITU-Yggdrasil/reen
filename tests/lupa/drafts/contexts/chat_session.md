# Context: ChatSession

## Purpose
Manages an in-process conversational agent that helps the authenticated customer interact with their banking services through natural language. The agent's tool set is limited to the five business contexts (list accounts, show transactions, own-account transfer, domestic transfer, international transfer). Mutating actions are never executed directly by the agent — instead, they produce confirmation cards that the customer must explicitly confirm.

## Role Players
- authenticated_customer: AuthenticatedCustomer — the verified customer identity participating in the conversation
- session: Session carrying ChatConversation — the session containing the ongoing conversation history
- agent: @chat-llm — the language model that interprets customer messages and selects tools
- tool_registry: the registry of available tools, constrained to the five business contexts
- audit_sink: AuditSink — receives audit records for every conversation turn and tool invocation

## Role Methods
### authenticated_customer
- provide_context: supplies customer identity and granted scopes so the agent knows what operations are available

### session
- load_conversation: retrieves the current conversation history from the session
- append_turn: adds the customer message and agent response to the conversation history
- persist: saves the updated conversation back to the session

### agent
- interpret: processes the customer's message in the context of conversation history and available tools, producing a response that may include tool invocations
- render_confirmation_card: when the agent determines a mutating action is appropriate, it produces a confirmation card describing the proposed transfer instead of executing it

### tool_registry
- resolve_tool: maps an agent tool invocation to the corresponding business context, confirming the tool is in the allowed set
- check_scope: verifies the customer's scopes permit the requested tool

### audit_sink
- record: appends an audit entry for each conversation turn, including any tool invocations and whether confirmation cards were issued

## Functionalities
### converse
The session's load_conversation retrieves the conversation history. The authenticated_customer's provide_context supplies the customer identity and scopes. The agent's interpret processes the customer's message, referencing conversation history and the tool_registry's available tools. If the agent invokes a read-only tool (list accounts, show transactions), the tool_registry's resolve_tool and check_scope validate the call, and the result is incorporated into the agent's response. If the agent determines a mutating action is needed, the agent's render_confirmation_card produces a confirmation card instead of executing the transfer. The session's append_turn records the exchange and the session's persist saves it. The audit_sink's record logs the turn. The agent never executes a mutating operation directly.

## Scenarios
### customer asks to see their accounts
Given: authenticated customer C-9281 holds scopes accounts-read and transactions-read, session S-00712 has an empty conversation history
When: converse is invoked with customer message "Show me my accounts"
Then: the agent invokes the list accounts tool, returns the account list in natural language ("You have two accounts: current account ending 0785 with 12450.00 EUR and savings account ending 0786 with 3200.00 EUR"), the conversation history is updated, and an audit entry records "chat turn: tool invocation list-accounts" for customer C-9281

### customer requests a domestic transfer — confirmation card issued
Given: authenticated customer C-9281 holds scope payments-initiate-domestic, session S-00712 has prior conversation context
When: converse is invoked with customer message "Transfer 89.90 EUR to Elisa Oyj account 1234-56789012 for invoice 2026-04-001"
Then: the agent produces a confirmation card describing the proposed domestic transfer of 89.90 EUR to 1234-56789012, recipient name "Elisa Oyj", message "Invoice 2026-04-001", the response asks the customer to confirm, the conversation history is updated, and an audit entry records "chat turn: confirmation card issued for domestic transfer" for customer C-9281

### customer asks about something outside available tools
Given: authenticated customer C-9281 with session S-00712
When: converse is invoked with customer message "What is the weather in Helsinki?"
Then: the agent responds that it can only help with banking operations (viewing accounts, viewing transactions, and initiating transfers), no tool is invoked, and an audit entry records "chat turn: out-of-scope query" for customer C-9281
