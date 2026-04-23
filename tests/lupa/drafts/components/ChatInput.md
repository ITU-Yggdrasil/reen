# Component: ChatInput

## Purpose

A multi-line text entry field paired with a send button, used to compose messages in the chat interface. Supports keyboard submission and disables the send control when the field is empty or a message is in flight.

## Props

- value: the current text content of the input field
- sending: whether a message submission is currently in progress, defaults to false

## Events

- message:submit: fired when the user activates the send control or uses the keyboard shortcut, carries the current text content

## Invariants

- The send control must be disabled when value is empty or consists only of whitespace
- The send control must be disabled while sending is true
- After a successful submission, value must clear to empty

## Scenarios

- User types a message and taps send: message:submit fires with the text, the field clears, and the send control enters the sending state
- User presses the keyboard submit shortcut: same behaviour as tapping send
- User attempts to send an empty message: the send control is disabled, nothing happens
- A submission is in progress: the send control shows a busy state and additional submissions are blocked
