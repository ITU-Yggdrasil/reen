# Component: ChatMessage

## Purpose

Renders a single message within a conversation thread. Adapts its appearance based on the message origin: user-authored, agent-authored, or a tool result returned by the system.

## Props

- origin: one of user, agent, or tool-result
- content: the textual body of the message
- timestamp: when the message was created

## Variants

- user: aligned to the trailing edge, displayed on an accent-action background with surface-neutral text
- agent: aligned to the leading edge, displayed on surface-elevated with text-primary
- tool-result: aligned to the leading edge, displayed on surface-elevated with mono-numeric typography and a subtle border to distinguish it from conversational messages

## Invariants

- Messages must appear in chronological order within their parent thread
- Tool-result messages must never be editable or re-sendable
- Timestamps display in text-secondary using a relative format when recent and an absolute format otherwise

## Scenarios

- User sends a message: a new bubble appears at the trailing edge with the current timestamp
- Agent responds: a bubble appears at the leading edge in the agent style
- System returns a tool result such as a balance lookup: a tool-result bubble appears with the data formatted in mono-numeric typography
