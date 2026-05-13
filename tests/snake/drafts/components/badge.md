# Badge - Component Specification

## Component Metadata

### Name

Badge

### Description

A Badge is a compact visual label used to indicate status, category, or priority. It should be used for short, high-signal markers that support a larger component without taking over the hierarchy.

---

## Visual Structure

The Badge is a compact inline element with short text and optional emphasis styling.

### Layout Structure

Inline and compact, with content centered inside a small rounded surface.

### Content Areas or Slots

- **Content slot (required):** Short text or label content.

### Alignment and Spacing Rules

- Content is centered vertically and horizontally.
- Padding remains tight so the badge stays secondary to surrounding content.

---

## Variants

- **Neutral:** Uses subdued styling for general labels.
- **Success:** Used for positive or complete states.
- **Warning:** Used for cautionary states.
- **Destructive:** Used for error or high-risk states.

### Brand Constraints

Use the project’s active visual/brand specification to decide the badge’s colors and typography. Do not introduce new token paths in the specification.

- **Neutral:** white.
- **Success:** green.
- **Warning:** blue with restrained emphasis.
- **Destructive:** red , since no negative/error semantic token is defined.

---

## States

### Default

The badge is visible and stable.

### Disabled

The badge appears muted when it should not draw emphasis.

---

## Properties

- `label`: String. Required. The badge text.
- `variant`: `neutral` | `success` | `warning` | `destructive`.

---

## Accessibility Notes

### ARIA Roles and Accessibility Considerations

- The badge should be read as text unless it conveys a meaningful state that needs additional announcement.
