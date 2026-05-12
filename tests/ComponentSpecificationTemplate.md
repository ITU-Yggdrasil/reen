# [Component Name] - Component Specification

## Component Metadata

- **Name:** [Exact component name]
- **Description:** [1-3 sentences describing what this component is for and when to use it]

---

## Visual Structure

- **Layout structure:** [Inline, stacked, horizontal, vertical, centered, etc.]
- **Content areas or slots:** [List the visible regions or say `_None._`]
- **Alignment and spacing rules:** [Short rules for spacing, alignment, and visual balance]

Optional if this component is composed of other library components:
- **Subcomponents:**
  - [ComponentName]: [Role in this component]
  - [ComponentName]: [Role in this component]

---

## Variants

List only the variants that actually exist.

- **[Variant name]:** [What it is and when to use it]
- **[Variant name]:** [What changes visually or behaviorally]

If there are no variants, write exactly `_None._`.

---

## States

List only the states that actually exist.

- **Default:** [Baseline appearance/behavior]
- **Hover:** [Only include if defined]
- **Active:** [Only include if defined]
- **Disabled:** [Only include if defined]
- **Loading:** [Only include if defined]
- **[Custom state]:** [Description]

If there are no distinct states beyond default, keep only `Default`.

---

## Properties

List every configurable input in plain language first.

- `prop_name`: [What it controls, whether it is required, and what kind of value it accepts]
- `prop_name`: [Allowed values if limited, or object/list summary if structured]

If the component has no configurable props, write exactly `_None._`.

---

## Implementation Contract

This section is required because the system depends on it.

### Props

Write one bullet per configurable prop using this exact pattern:

```md
- `prop_name`: required=`true|false`; shape=`scalar|enum|object|list`; type=`TypeName`
```

Use these add-ons only when needed:

- `; allowed=\`value-a|value-b|value-c\`` for `shape=enum`
- `; object_contract=\`Name\`` for `shape=object`
- `; item_contract=\`Name\`` for `shape=list`

Example:

```md
- `label`: required=`true`; shape=`scalar`; type=`String`
- `variant`: required=`true`; shape=`enum`; type=`String`; allowed=`neutral|success|warning|destructive`
- `items`: required=`false`; shape=`list`; type=`NavItem`; item_contract=`NavItem`
```

### Object Contracts

Use this only for structured object values.

```md
#### `ContractName`
- `field_name`: required=`true|false`; shape=`scalar|enum|object|list`; type=`TypeName`
```

If none are needed, write exactly `_None._`.

### Collection Contracts

Every prop with `shape=list` must appear here.

```md
- `prop_name`: item_contract=`Name`; behavior=`repeated-item`
```

If none are needed, write exactly `_None._`.

### Interaction Contracts

List only implementation-relevant interactions.

```md
- `target`: kind=`navigational|callback-driven|stateful`
```

Use exact targets such as `href`, `on_click`, `items[*]`, or `theme_toggle.options[*]`.

If none are needed, write exactly `_None._`.

### Composition Contracts

Use this when the component depends on other named components.

```md
- `ComponentName`: usage=`required|optional|slot-provided|reused-subcomponent`
```

If none are needed, write exactly `_None._`.

### Brand Constraints

Include only when the active brand or visual spec materially affects implementation.

```md
- `topic`: Typography must use `brand.typography...`
- `topic`: Color must use `brand.colors...`
```

Rules:

- Keep each item to one flat bullet
- Use only tokens that actually exist in the active brand specs
- Start common topics with `Typography must ...`, `Color must ...`, `Spacing must ...`, or `Hierarchy must ...`

If none are needed, write exactly `_None._`.

---

## Accessibility Notes

- **Keyboard interaction expectations:** [Tab, Enter, Space, Arrow keys, or `_None._`]
- **ARIA roles and accessibility considerations:** [Semantic element, role, labels, announcements, or `_None._`]

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

- Keep this document focused on one reusable component only
- Do not invent variants, states, props, tokens, or accessibility behavior
- Use exact authored names for component names, prop names, variant names, and token names
- If structured props or repeated items exist, define them clearly enough that implementation does not need to guess
