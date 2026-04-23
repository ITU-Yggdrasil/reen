# Component: ChatInput

## Purpose

A multi-line text entry field paired with a send button, used to compose messages in the chat interface. Supports keyboard submission and disables the send control when the field is empty or a message is in flight.

## Props

- value: the current text content of the input field
- sending: whether a message submission is currently in progress, defaults to false

## Events

- message-submit: fired when the user activates the send control or uses the keyboard shortcut, carries the current text content

## Invariants

- The send control must be disabled when value is empty or consists only of whitespace
- The send control must be disabled while sending is true
- After a successful submission, value must clear to empty

## Scenarios

### user sends a message by tapping send
Given: the ChatInput contains the text "What is my balance?"
When: the user taps the send control
Then: message-submit fires with "What is my balance?", the field clears, and the send control enters the sending state

### empty input disables send
Given: the ChatInput value is empty
When: the user views the send control
Then: the send control is disabled and tapping it has no effect
