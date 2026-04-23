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

- message:submit: propagated from ChatInput when the user sends a message, carries the text content

## Invariants

- The pane must auto-scroll to the newest message when a new message arrives, unless the user has manually scrolled upward to review history
- Messages must render in chronological order, oldest at the top
- The ChatInput must remain visible and anchored at the bottom regardless of scroll position

## Scenarios

- User sends a message: the message appears immediately as a user-origin ChatMessage, the pane scrolls to the bottom, and the ChatInput clears
- Agent responds: a new agent-origin ChatMessage appears and the pane scrolls to the bottom if the user has not scrolled away
- A tool result arrives: a tool-result ChatMessage appears in the conversation at its chronological position
- User scrolls up to review earlier messages then a new message arrives: the pane does not auto-scroll, preserving the user's reading position
