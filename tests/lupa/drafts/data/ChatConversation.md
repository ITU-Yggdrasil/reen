# Data: ChatConversation

## Purpose
Represents the ordered sequence of exchanges between a customer and the banking agent within a single session. A ChatConversation accumulates ChatTurns as the dialogue progresses, preserving the full history needed for context-aware agent responses and audit compliance.

## Attributes
- turns: an ordered list of ChatTurn values, from earliest to most recent
- session reference: a reference to the Session this conversation belongs to

## Invariants
- The turns list preserves insertion order and is append-only within a conversation
- A conversation must belong to exactly one session
- The turns list may be empty at the start of a conversation

## Operations
### append turn
Adds a new ChatTurn to the end of the turns list. The turn's position in the sequence is determined by its insertion order.

### last turn
Retrieves the most recent ChatTurn in the conversation, or reports absent if the conversation has no turns.

## Scenarios
### New conversation with no turns
Given: session reference "SESSION-001" and an empty turns list
When: a ChatConversation is created
Then: the ChatConversation is accepted with zero turns

### Append a user utterance turn
Given: a ChatConversation with session reference "SESSION-001" and zero turns
When: append turn is performed with a user-utterance ChatTurn containing "What is my balance?"
Then: the conversation has one turn and the last turn is the user utterance

### Append multiple turns
Given: a ChatConversation with one user-utterance turn "What is my balance?"
When: append turn is performed with an agent-response ChatTurn containing "Your current account balance is 1,500.00 DKK."
Then: the conversation has two turns in order: the user utterance followed by the agent response

### Last turn of empty conversation is absent
Given: a ChatConversation with zero turns
When: last turn is queried
Then: the result is absent
