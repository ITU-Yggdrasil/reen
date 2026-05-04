# ChatPanel - Component Specification

## Component Metadata

### Name

ChatPanel

### Description

ChatPanel is the assistant workspace component used to support account questions, recent-activity lookups, and transfer-oriented guidance alongside the main dashboard. It should be used as a supportive side panel that helps users act on their accounts without replacing the account overview itself.

---

## Visual Structure

ChatPanel is a bordered vertical panel composed of a title area, a conversation or empty-state region, and a bottom-aligned input composer with a send action.

### Layout Structure

Vertical stack with three regions: header, body, and composer. The body should expand to occupy the main panel height while the composer remains anchored near the bottom edge.

### Subcomponents

- `Layout-Containers`: used to create the panel shell, body region, and composer row.
- `Heading`: used for the panel title.
- `Text`: used for helper copy, empty-state messaging, and conversation text.
- `Button`: used for the send action.
- `Input`: used for the message composer.
- `Icon`: optional, used for the empty-state conversation marker.

### Content Areas or Slots

- **Header slot (required):** The panel title.
- **Conversation slot (required):** The message history region or empty-state body.
- **Empty state slot (optional but expected):** Guidance shown before the first conversation message.
- **Composer slot (required):** Input field and send action row.

### Alignment and Spacing Rules

- The panel title should stay close to the top edge but separated clearly from the bordered body.
- The conversation region should feel open and breathable, especially in the empty state.
- The composer row should align its input and send action on a shared baseline.
- The send action should remain visually connected to the input without dominating the panel.

---

## Variants

- **Default:** Standard dashboard assistant panel.
- **Empty State:** No messages yet; shows guidance and starter intent.
- **Conversation Active:** Existing message history is visible above the composer.

---

## States

### Default

The panel is visible with title, body, and composer ready for use.

### Empty

The body shows an icon or illustration cue and short guidance text encouraging the first prompt.

### Typing / Submitting

The composer reflects in-progress submission without losing the current layout.

### Disabled

The input and send action are unavailable while preserving the panel shell.

---

## Properties

- `title`: String. Required. The panel heading.
- `empty_state_heading`: String. Optional. Main empty-state prompt.
- `empty_state_description`: String. Optional. Supporting guidance text.
- `placeholder`: String. Required. Placeholder text for the input field.
- `send_label`: String. Required. Visible label for the send action.
- `messages`: List. Optional. Existing conversation items.
- `variant`: `default` | `empty-state` | `conversation-active`.
- `disabled`: Boolean. Optional. Whether the composer is currently unavailable.
- `submitting`: Boolean. Optional. Whether the panel is actively sending a message.

---

## Accessibility Notes

### Keyboard Interaction Expectations

- The input field and send action must be reachable in natural order.
- Users should be able to type in the composer and submit with a clear keyboard pattern.
- Any disabled or submitting state must be communicated without trapping focus.

### ARIA Roles and Accessibility Considerations

- The panel should expose a clear section label via its heading.
- Conversation or status messaging should be structured so updates can be perceived by assistive technologies when appropriate.
- Placeholder text should not be the only source of instruction if the panel needs persistent guidance.

---

## Usage Guidelines

### Do

- Use concise, task-oriented empty-state guidance.
- Keep the panel visually supportive rather than dominant.
- Preserve a stable body-and-composer structure across empty and active states.

### Don't

- Don't overload the side panel with unrelated product modules.
- Don't make the empty state so decorative that it distracts from the accounts overview.
- Don't detach the send action too far from the input field.
