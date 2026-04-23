# Context: ChatSession

## Purpose
Manages an in-process conversational agent that helps the authenticated customer interact with their banking services through natural language. The agent's capabilities are limited to the five business contexts (list accounts, show transactions, own-account transfer, domestic transfer, international transfer). Mutating actions are never executed directly by the agent — instead, they produce confirmation cards that the customer must explicitly confirm.

## Role Players
- authenticated_customer: AuthenticatedCustomer — the verified customer identity participating in the conversation
- session: Session — the session containing the ongoing conversation history
- conversation: ChatConversation — the ordered history of turns in this conversation

## Props
- customer_message: the natural-language message submitted by the customer in this turn

## Role Methods
### authenticated_customer
- provide_context: supplies customer identity and granted scopes so the agent knows what operations are available

### session
- persist: saves the updated conversation back to the session

### conversation
- load_history: retrieves the ordered list of prior turns for context
- append_turn: adds the customer message and agent response as a new turn

## Functionalities
### converse
The conversation's load_history retrieves the prior turns. The authenticated_customer's provide_context supplies the customer identity and scopes. The customer message is interpreted in the context of conversation history and the available banking operations. If a read-only operation is appropriate (list accounts, show transactions), the corresponding business context is invoked and the result is incorporated into the response. If a mutating action is needed, a ConfirmationCard describing the proposed transfer is produced instead of executing the transfer. The conversation's append_turn records the exchange and the session's persist saves it. The AuditRecording context's record logs the turn. The agent never executes a mutating operation directly.

## Scenarios
### customer asks to see their accounts
Given: authenticated customer C-9281 holds scopes accounts-read and transactions-read, session S-00712 has an empty conversation history
When: converse is invoked with customer message "Show me my accounts"
Then: the list accounts operation is invoked, the response includes the account list in natural language ("You have two accounts: current account ending 0785 with 12450.00 EUR and savings account ending 0786 with 3200.00 EUR"), the conversation history is updated, and an audit entry records "chat turn: tool invocation list-accounts" for customer C-9281

### customer requests a domestic transfer — confirmation card issued
Given: authenticated customer C-9281 holds scope payments-initiate-domestic, session S-00712 has prior conversation context
When: converse is invoked with customer message "Transfer 89.90 EUR to Elisa Oyj account 1234-56789012 for invoice 2026-04-001"
Then: a confirmation card describing the proposed domestic transfer of 89.90 EUR to 1234-56789012, recipient name "Elisa Oyj", message "Invoice 2026-04-001" is produced, the response asks the customer to confirm, the conversation history is updated, and an audit entry records "chat turn: confirmation card issued for domestic transfer" for customer C-9281

### customer asks about something outside available tools
Given: authenticated customer C-9281 with session S-00712
When: converse is invoked with customer message "What is the weather in Helsinki?"
Then: the response explains it can only help with banking operations (viewing accounts, viewing transactions, and initiating transfers), no business context is invoked, and an audit entry records "chat turn: out-of-scope query" for customer C-9281
