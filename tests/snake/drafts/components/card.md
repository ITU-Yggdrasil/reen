# Card - Component Specification

## Component Metadata

### Name

Card

### Description

A Card is a composed surface component used to group related content, status, and actions into a single readable unit. It should be used for previews, summaries, feature highlights, and modular content blocks where a clear boundary helps scanning and comparison. This component is intentionally defined as a larger container that relies on existing library components for many of its internal building blocks.

---

## Visual Structure

The Card is a bounded container with a clear outer surface, internal padding, and a predictable vertical reading order. A typical card includes a media or visual accent area, a header group, a body content area, optional metadata, and a footer action area. Depending on context, the card may present as a static information block or as an interactive summary that links to more detail.

### Layout Structure

Primarily vertical and stacked. The default layout follows a top-to-bottom structure with optional horizontal arrangements inside local regions such as metadata rows or action groups. Internal grouping should use existing layout container patterns rather than inventing one-off spacing behavior.

### Subcomponents

- `Layout-Containers`: used to organize the card into stacked, row, grid, and section-like internal regions.
- `Button`: used for primary or secondary actions in the footer or action row.
- `Heading`: used for the main title and optional section labels inside the card.
- `Text`: used for supporting copy, labels, and small metadata strings.
- `Link`: used when the card exposes secondary navigation or inline destinations.
- `Badge`: used for optional status, category, or priority markers.
- `Image`: used for optional preview imagery, illustration, or thumbnail content.

### Content Areas or Slots

Include:

- **Media slot (optional):** A top or side-aligned region for imagery, illustration, thumbnail content, or a decorative preview surface.
- **Header slot (required):** The primary identification area containing the card title and optional status or category marker.
- **Body slot (required):** Supporting description, summary content, or preview information that explains the card's purpose.
- **Metadata slot (optional):** Compact supporting details such as date, author, tags, counts, or status text.
- **Action slot (optional):** A region for one primary action and, when needed, one or more secondary actions.

### Alignment and Spacing Rules

- Content is aligned to a consistent internal grid with shared left and right padding.
- Vertical spacing should separate regions clearly: header to body is tighter than body to action area.
- Metadata rows and action rows may use horizontal layout containers with consistent gaps between items.
- If media is present, its relationship to the content block should feel intentional: edge-to-edge within the card frame or inset as part of the card padding system.
- Text should align to the same primary content edge even when badges, icons, or actions are present.
- Footer actions should align as a group and should not visually compete with the title or summary content.

---

## Variants

- **Informational:** Used for summaries, read-only previews, and modular content blocks where scanning matters more than immediate action. Uses restrained emphasis and balanced spacing.
- **Interactive:** Used when the card itself is clickable or acts as a prominent navigation target. The whole surface communicates affordance while preserving clear internal hierarchy.
- **Feature:** Used for highlighted content, promotions, or important surfaced content. May use stronger media presence, larger spacing, or more prominent headline treatment.
- **Compact:** Used in dense lists or dashboards where the same card pattern must repeat in limited space. Reduces padding and truncates secondary detail before reducing title clarity.
- **Status:** Used when the card needs to foreground a badge, state, or workflow condition such as success, warning, draft, or blocked.

For each variant, clarify:

- Informational is best when the content should feel neutral and readable.
- Interactive differs by emphasizing hover, focus, and click affordance across the full surface.
- Feature differs by increasing visual prominence through media, spacing, or stronger headline hierarchy.
- Compact differs by compressing layout rhythm while retaining recognizable regions.
- Status differs by making state communication more prominent than decorative content.

---

## States

### Default

The card surface is visible and stable, with all regions rendered at full opacity. The hierarchy should be immediately legible, with the title as the dominant entry point and supporting content arranged beneath it.

### Hover

For interactive cards, the surface may lift slightly through shadow, border contrast, or background shift. Hover feedback should apply to the container without causing layout shift. For non-interactive cards, hover should not imply clickability.

### Active

Interactive cards may show a pressed state through reduced elevation, subtle scale change, or darker surface treatment. If the card contains internal buttons or links, their active states remain independent and should not visually conflict with the card's own press feedback.

### Disabled

The card appears unavailable through reduced contrast and suppressed affordance. Interactive behaviors are blocked. Any internal actions inside a disabled card should also be unavailable or omitted.

### Loading (if applicable)

Loading cards preserve the final layout footprint to avoid reflow in lists or grids. Placeholder regions may stand in for media, title, text, metadata, and actions. The overall structure should remain recognizable even while content is not yet available.

---

## Properties

- `title`: String. Required. The primary heading for the card.
- `description`: String or rich text summary. Optional but expected in most informational uses.
- `variant`: `informational` | `interactive` | `feature` | `compact` | `status`.
- `media`: Optional media content, image reference, or illustrative surface.
- `badge`: Optional status, category, or emphasis label.
- `metadata`: Optional list or grouped set of supporting details.
- `actions`: Optional action set, typically one primary action plus secondary actions or links.
- `orientation`: `vertical` | `horizontal`. Controls whether media and content stack or sit side by side.
- `interactive`: Boolean. When true, the whole card behaves as a target and should expose hover, active, and focus treatment.
- `selected`: Boolean. Optional. Indicates the card is currently chosen within a set.
- `disabled`: Boolean. Optional. Prevents interaction and applies disabled styling.
- `loading`: Boolean. Optional. Replaces content with loading placeholders while preserving size and structure.

---

## Accessibility Notes

### Keyboard Interaction Expectations

- If the whole card is interactive, it must be reachable via `Tab` as a single focus target unless the pattern intentionally exposes multiple internal controls.
- `Enter` and `Space` should activate the card when it behaves like a button-like surface.
- Internal buttons and links must remain reachable in a predictable focus order when present.
- If both the card surface and internal actions are interactive, focus behavior should avoid duplicate or confusing activation paths.

### ARIA Roles and Accessibility Considerations

- Non-interactive cards should usually render as semantic grouping content such as `<section>`, `<article>`, or `<div>` depending on context.
- Interactive cards should use the semantic element that best matches behavior, such as a link for navigation or a button-like pattern for in-place actions.
- The card must expose a clear accessible name, typically derived from the title.
- Status badges, counts, and metadata should be announced in a meaningful order and should not replace the title as the primary accessible identifier.
- Loading cards should communicate busy or updating status when the card updates asynchronously.

---

## Optional: Usage Guidelines and Examples

### Do

- Use cards when content benefits from a visible boundary and predictable internal structure.
- Reuse established library components inside the card instead of redefining bespoke child patterns.
- Keep one dominant entry point, usually the title or primary action, so the card remains easy to scan.
- Use compact variants for repeated collections and richer variants for featured or editorial content.

### Dont

- Don't overload a single card with too many competing actions.
- Don't make a card appear clickable unless the full-surface interaction is intentional.
- Don't mix unrelated content types inside one card just because a container is available.
- Don't redefine internal button or layout behavior in the card spec when existing component drafts already cover those patterns.

---

## Notes

- This document defines **one component specification** and should stay focused on that component only.
- Internal child behavior should defer to existing component drafts where those components are already defined in the library.
- Keep guidance implementation-agnostic unless framework-specific behavior is explicitly required.
- Prioritize consistency across variants, states, and accessibility behavior.
