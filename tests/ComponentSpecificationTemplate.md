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

### Subcomponents
[List the named design library components that this component is composed of, if any. Omit this section for atomic components like Button or Badge.]

- [Subcomponent name]: [its role within this component]
- [Subcomponent name]: [its role within this component]

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

## Implementation Contract

[Capture the implementation-driving contract in a compact, machine-readable form.]

### Props

[List every configurable prop using this exact bullet pattern.]

```md
- `prop_name`: required=`true|false`; shape=`scalar|enum|object|list`; type=`TypeName`
```

[Additional attributes:]

- add `; item_contract=\`Name\`` for `shape=list` when items are structured values
- add `; object_contract=\`Name\`` for `shape=object`
- add `; allowed=\`value-a|value-b|value-c\`` for `shape=enum`
- the bullet itself must be the contract line; do not wrap the whole bullet in extra backticks
- no valid contract line may begin with the literal prefix `- \`- \``

Valid example:

```md
- `items`: required=`false`; shape=`list`; type=`NavItem`; item_contract=`NavItem`
```

Invalid example:

```md
- `- `items`: required=`false`; shape=`list`; type=`NavItem`; item_contract=`NavItem``
```

### Object Contracts

[For every structured object-like prop or repeated item, define its fields.]

#### `ContractName`

- `field_name`: required=`true|false`; shape=`scalar|enum|object|list`; type=`TypeName`

[If none are needed, write exactly `_None._`.]

### Collection Contracts

[For every collection prop, define how repeated items are shaped.]

- `prop_name`: item_contract=`Name`; behavior=`repeated-item`

[Every prop with `shape=list` should appear here. If none are needed, write exactly `_None._`.]

### Interaction Contracts

[For every implementation-relevant interaction, define the interaction kind.]

- `target`: kind=`navigational|callback-driven|stateful`

[Use exact prop or repeated-item targets such as `brand_href`, `items[*]`, `utility_actions[*]`, `legal_links[*]`, or `theme_toggle.options[*]`.]
[Do not list plain display/configuration props such as `label`, `title`, or `size` as interactions unless the source explicitly defines them as interactive state controls.]
[Prefer exact prop or repeated-item targets over the component name itself whenever possible.]
[Do not use the component name itself as the interaction target when a more exact prop, event prop, href prop, action prop, or repeated-item target exists.]
[Do not append stray punctuation to the target; use `items[*]`, not `items[*])`.]
[If none are needed, write exactly `_None._`.]

### Composition Contracts

[List composed components that are required for the implementation shape.]

- `ComponentName`: usage=`required|optional|slot-provided|reused-subcomponent`

[If none are needed, write exactly `_None._`.]

### Brand Constraints

[Optional. Include only when active brand identity or visual specifications materially shape implementation choices.]

- `topic`: [Concrete brand-informed implementation guidance for hierarchy, spacing, typography, motion, iconography, or token usage.]

[The guidance text itself must explicitly name the implementation dimension it controls, such as `typography`, `spacing`, `color`, `hierarchy`, or `contrast`.]
[Prefer starting each brand-constraint sentence with the topic name itself, such as `Typography must ...`, `Color must ...`, `Hierarchy must ...`, or `Spacing must ...`.]
[For the common topics `typography`, `spacing`, `color`, and `hierarchy`, the guidance sentence should begin with the capitalized topic word itself.]
[Do not place nested bullets under a brand-constraint bullet; each constraint must remain one flat bullet line.]
[If multiple token rules are needed, express them in one sentence or use multiple flat `- `topic`: ...` bullets.]

[Valid examples:]
- `typography`: Typography must use `testcompany.typography.family.primary` for visible text in this component.
- `color`: Color must use `testcompany.colors.primary.red` for the primary emphasis treatment in this component.
- `hierarchy`: Hierarchy must keep the summary region visually dominant over secondary content.
- `spacing`: Spacing must preserve generous whitespace between repeated items to support the brand's low-clutter hierarchy.

[Invalid examples:]
- `**Typography**`: Use `testcompany.typography.family.primary` for visible text in this component.
- `Typography`: Use `testcompany.typography.family.primary` for visible text in this component.
- `typography`: Use `testcompany.typography.family.primary` for visible text in this component.
- `color`: The negative-balance variant must use `testcompany.colors.primary.red` for emphasis.
- `hierarchy`: The summary region must visually dominate the page.
- `color`: Use the following tokens:
  - Primary: `testcompany.colors.primary.red`

[If none are needed, write exactly `_None._`.]

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
- Finalized specifications should be implementation-driving: prop shapes, repeated-item schemas, object fields, and interaction kinds must not be left to downstream inference
