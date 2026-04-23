# Component: ChatPane

## Purpose

A scrollable conversation panel that combines a chronological list of ChatMessages with a ChatInput at the bottom. Serves as the primary interface for the user to interact with the banking agent. Binds to the ChatSession context for message delivery and receipt.

## Composition

- ChatMessage: one instance per message in the conversation history
- ChatInput: anchored at the bottom of the pane for composing new messages

## Props

- messages: the ordered collection of messages in the current session
- sending: whether a message submission is currently in progress

## Events

- message-submit: propagated from ChatInput when the user sends a message, carries the text content

## Invariants

- The pane must auto-scroll to the newest message when a new message arrives, unless the user has manually scrolled upward to review history
- Messages must render in chronological order, oldest at the top
- The ChatInput must remain visible and anchored at the bottom regardless of scroll position

## Scenarios

### user sends a message
Given: the ChatPane displays a conversation and the ChatInput contains "Transfer 100 EUR"
When: the user submits the message via the ChatInput
Then: message-submit fires with "Transfer 100 EUR", the message appears as a user-origin ChatMessage, and the pane scrolls to the bottom

### new message arrives while user has scrolled up
Given: the user has scrolled up in the ChatPane to review earlier messages
When: the agent sends a new response
Then: the agent-origin ChatMessage appears at the bottom but the pane does not auto-scroll, preserving the user's reading position
