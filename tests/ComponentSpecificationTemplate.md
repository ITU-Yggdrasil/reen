# [Component Name] - Component Specification

## Component Metadata

### Name

[Provide the unique identifier for this component.]

### Description

[Write 1-3 sentences describing the components purpose and when it should be used.]

---

## Visual Structure

[Describe the component's visual and structural composition.]

### Layout Structure

[Describe the base layout pattern (e.g., horizontal, vertical, stacked, inline).]

### Content Areas or Slots

[List and describe available content regions or slots.]

Include:

- [Slot/area 1 and what belongs there]
- [Slot/area 2]
- [Optional slot/area 3]

### Alignment and Spacing Rules

[Define alignment behavior and spacing intent.]

Include:

- [Internal spacing rules]
- [Spacing between child elements]
- [Alignment rules for text/icons/actions]

---

## Variants

[Describe supported visual variations for this component.]

Examples:

- [Variant 1 e.g., Primary]
- [Variant 2 e.g., Secondary]
- [Variant 3 e.g., Outlined]
- [Optional variant 4]

For each variant, clarify:

- [When to use it]
- [How it differs visually from other variants]

---

## States

[Describe how the component appears and behaves in each state.]

### Default

[Baseline appearance and behavior.]

### Hover

[Visual and interaction changes on pointer hover.]

### Active

[Visual and interaction changes when pressed/selected.]

### Disabled

[How it appears when unavailable and what interactions are blocked.]

### Loading (if applicable)

[How loading is communicated and whether interactions are temporarily disabled.]

---

## Properties

[List configurable inputs and accepted value types/options.]

Include common properties such as:

- `label`: [Text content, optional/required]
- `icon`: [Supported icon positions or icon-only usage]
- `size`: [Available sizes e.g., small, medium, large]
- `variant`: [Maps to visual style variants]

Optional additional properties:

- [Property name]: [Purpose and valid values]
- [Property name]: [Purpose and valid values]

---

## Accessibility Notes

[Document accessibility expectations relevant to this component.]

### Keyboard Interaction Expectations

[Describe focus behavior and keyboard controls (e.g., Tab, Enter, Space, Arrow keys where relevant).]

### ARIA Roles and Accessibility Considerations

[Define role/ARIA usage and semantic requirements if relevant.]

Consider:

- [Required role or semantic element]
- [Accessible name/label requirements]
- [State announcements (e.g., disabled, expanded, loading)]

---

## Optional: Usage Guidelines and Examples

[Explain practical usage guidance and implementation intent.]

### Do

- [Best practice 1]
- [Best practice 2]
- [Best practice 3]

### Dont

- [Common mistake 1]
- [Common mistake 2]
- [Common mistake 3]

[Optional: Include concise examples of correct vs. incorrect usage contexts.]

---

## Notes

- This document defines **one component specification** and should stay focused on that component only
- Keep guidance implementation-agnostic unless framework-specific behavior is explicitly required
- Prioritize consistency across variants, states, and accessibility behavior
