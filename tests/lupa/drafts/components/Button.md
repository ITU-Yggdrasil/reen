# Component: Button

## Purpose

The standard interactive control for triggering actions. Communicates importance through its variant and disables itself while an action is in flight to prevent duplicate submissions.

## Props

- label: the visible text describing what the button does
- variant: one of primary, secondary, or destructive
- disabled: whether the button is non-interactive, defaults to false
- busy: whether an action is currently in progress, defaults to false

## Variants

- primary: filled background in accent-action with text in surface-neutral, used for the single most important action on a screen
- secondary: outlined in accent-action with transparent background, used for alternative or supporting actions
- destructive: filled background in accent-destructive with text in surface-neutral, used for irreversible operations such as cancellation

## Invariants

- A screen must never present more than one primary button at a time
- When busy is true, the button must be visually distinct from disabled and must prevent additional activations
- The destructive variant must only appear inside a confirmation flow

## Scenarios

- User taps a primary button: the action fires and the button enters the busy state until the action completes
- User taps a disabled button: nothing happens, no event is emitted
- User taps a destructive button outside a confirmation context: this scenario must not occur per the invariant above
