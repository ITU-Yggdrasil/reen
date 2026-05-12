# [Component Name] - Component Specification

## Component Metadata

### Name

[Component name]

### Description

[Write 1-3 sentences describing what this component is, what it is for, and when it should be used.]

---

## Visual Structure

[Describe the component as a whole in a short paragraph.]

### Layout Structure

[Describe the overall layout pattern, such as inline, stacked, horizontal, vertical, centered, or grid-like.]

### Subcomponents

[Optional. Include this section only if the component is composed from other named library components.]

- `[ComponentName]`: [What it is used for inside this component]
- `[ComponentName]`: [What it is used for inside this component]

### Content Areas or Slots

- **[Slot name] (required/optional):** [What belongs here]
- **[Slot name] (required/optional):** [What belongs here]

### Alignment and Spacing Rules

- [How content should align]
- [How spacing should behave between regions or child elements]
- [Any important balance, hierarchy, or padding guidance]

---

## Variants

- **[Variant name]:** [What it is for and how it differs]
- **[Variant name]:** [What it is for and how it differs]

[Optional: If the component relies on brand-specific visual treatment, describe that in plain language here without inventing tokens.]

---

## States

### Default

[Describe the baseline appearance and behavior.]

### Hover

[Include only if this state exists.]

### Active

[Include only if this state exists.]

### Disabled

[Include only if this state exists.]

### Loading (if applicable)

[Include only if this state exists.]

### [Custom State]

[Include only if needed.]

---

## Properties

- `prop_name`: [Type, whether it is required or optional, and what it controls]
- `prop_name`: [Allowed values if limited]
- `prop_name`: [List/object summary if it is structured]

---

## Accessibility Notes

### Keyboard Interaction Expectations

- [How keyboard users should reach and use it]
- [Keys that should work, if relevant]

### ARIA Roles and Accessibility Considerations

- [Semantic element or role guidance]
- [Accessible name, labels, announcements, or state communication]

---

## Optional: Usage Guidelines and Examples

### Do

- [Best practice]
- [Best practice]

### Dont

- [Common mistake]
- [Common mistake]

---

## Notes

- This document defines **one component specification** and should stay focused on that component only.
- Keep guidance implementation-agnostic unless framework-specific behavior is explicitly required.
- Prioritize consistency across variants, states, and accessibility behavior.
- The final generated specification can add the implementation contract later; this draft should focus on the human-authored component intent.
