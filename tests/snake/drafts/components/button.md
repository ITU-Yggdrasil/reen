# Button - Component Specification

## Component Metadata

### Name

Button

### Description

A Button is an interactive element that triggers an action or event, such as submitting a form, opening a dialog, or navigating to a new view. It should be used whenever a user needs to perform a discrete, intentional action. Buttons communicate affordance through their visual weight and labeling, and should always have a clear, descriptive label.

---

## Visual Structure

The Button is an atomic component with a fixed inline layout containing an optional leading icon, a text label, and an optional trailing icon.

### Layout Structure

Horizontal and inline. Content is centered both vertically and horizontally within a fixed-height container. The button stretches to fit its label by default, with an optional full-width mode.

### Content Areas or Slots

- **Leading icon slot (optional):** An icon placed to the left of the label, used to reinforce the action (e.g., a plus icon for "Add item").
- **Label slot (required):** The primary text describing the action. Should be short, verb-led, and descriptive.
- **Trailing icon slot (optional):** An icon placed to the right of the label, typically used to indicate direction or expansion (e.g., a chevron for a dropdown trigger).

### Alignment and Spacing Rules

- Label and icons are vertically centered within the button container.
- Consistent horizontal padding is applied on both sides of the content; padding scales with button size.
- A fixed gap separates the leading icon from the label, and the label from the trailing icon.
- In full-width mode, the content group (icon + label + icon) remains centered within the expanded container.

---

## Variants

- **Primary:** The highest visual weight. Used for the single most important action on a page or within a section (e.g., "Save", "Submit", "Get started"). Typically a solid filled background using the brand's primary color.
- **Secondary:** Medium visual weight. Used for supporting actions that are important but not the primary focus (e.g., "Cancel", "Edit"). Typically a muted fill or a subdued color to sit alongside a Primary button without competing.
- **Outlined:** Low visual weight with a visible border and transparent background. Used for tertiary actions or in contexts where a lighter visual presence is needed (e.g., "Learn more", "View details").
- **Ghost:** Minimal visual weight with no border and no background fill. Used for low-priority inline actions where a button boundary would feel visually heavy (e.g., icon buttons in toolbars, inline text actions).
- **Destructive:** Signals a dangerous or irreversible action such as deletion. Uses a red or error-state color treatment. Can be applied as a filled or outlined style depending on the action's prominence.

---

## States

### Default

The button is at rest and ready for interaction. Displays its full color, label, and any icons at full opacity.

### Hover

Background color shifts to a slightly darker or lighter tone (depending on variant) to signal interactivity. Cursor changes to `pointer`. No layout shift occurs.

### Active

A further darkened or pressed appearance is applied while the pointer is held down, giving tactile feedback. Slight scale reduction (e.g., 98%) may be applied to reinforce a "pressed" feel.

### Disabled

The button becomes non-interactive. Opacity is reduced (typically to 40–50%) and the cursor changes to `not-allowed`. All click and keyboard events are blocked. The button should not receive focus when disabled.

### Loading (if applicable)

The label is replaced by or accompanied with a spinner indicator. The button remains visually present but interactions are temporarily disabled to prevent duplicate submissions. Width should remain stable to avoid layout shift.

---

## Properties

- `label`: String. Required. The visible action text displayed inside the button.
- `variant`: `primary` | `secondary` | `outlined` | `ghost` | `destructive`. Maps to the visual style variants described above.
- `size`: `small` | `medium` | `large`. Controls height, padding, and font size. Medium is the default.
- `icon-leading`: Icon reference. Optional. Renders an icon to the left of the label.
- `icon-trailing`: Icon reference. Optional. Renders an icon to the right of the label.
- `icon-only`: Boolean. Optional. Hides the label and renders only an icon; requires an accessible label to be provided separately.
- `disabled`: Boolean. Optional. Puts the button into its disabled state.
- `loading`: Boolean. Optional. Puts the button into its loading state.
- `full-width`: Boolean. Optional. Expands the button to fill the width of its container.
- `type`: `button` | `submit` | `reset`. Maps to the HTML button type attribute. Defaults to `button`.

---

## Accessibility Notes

### Keyboard Interaction Expectations

- The button must be focusable via `Tab` and included in the natural focus order.
- `Enter` and `Space` both trigger the button's action.
- Disabled buttons must not receive keyboard focus.
- Loading buttons should retain focus but block action re-triggering.

### ARIA Roles and Accessibility Considerations

- The button should render as a native `<button>` element where possible to inherit built-in role and keyboard behavior.
- If rendered as a non-native element (e.g., `<div>`), `role="button"` and `tabindex="0"` must be explicitly applied.
- **Accessible name:** Every button must have a discernible text label. For icon-only buttons, an `aria-label` or visually hidden text must be provided.
- **Disabled state:** Use the native `disabled` attribute on `<button>` elements. If using `aria-disabled`, ensure click and keyboard events are manually suppressed.
- **Loading state:** Use `aria-busy="true"` while loading. Consider adding a visually hidden status message (e.g., "Loading…") for screen reader users.

---

## Usage Guidelines

### Do

- Use a single Primary button per distinct action area to preserve visual hierarchy.
- Write labels using clear, verb-first language that describes the outcome (e.g., "Save changes", "Delete account").
- Pair a Primary button with a Secondary or Outlined button when offering a confirm/cancel choice.
- Use the Destructive variant for actions that cannot be undone, and consider pairing with a confirmation dialog.
- Maintain consistent button sizing within a single row or group of actions.

### Don't

- Don't use more than one Primary button in the same visual section — this undermines hierarchy and user decision-making.
- Don't use vague labels like "Click here", "OK", or "Yes" without sufficient surrounding context.
- Don't use a Ghost or Outlined button for a critical primary action where it might be overlooked.
- Don't disable a button without providing an explanation of why the action is unavailable, either via tooltip or contextual messaging.
- Don't resize buttons mid-flow during loading states — stabilize dimensions to prevent layout shift.

---

## Notes

- This document defines **one component specification** and should stay focused on that component only.
- Keep guidance implementation-agnostic unless framework-specific behavior is explicitly required.
- Prioritize consistency across variants, states, and accessibility behavior.