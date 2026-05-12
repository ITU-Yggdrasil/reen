number 1
# Card

## Component Metadata
- **Name**: Card
- **Description**: A Card is a composed surface component used to group related content, status, and actions into a single readable unit. It should be used for previews, summaries, feature highlights, and modular content blocks where a clear boundary helps scanning and comparison. This component is intentionally defined as a larger container that relies on existing library components for many of its internal building blocks.

---

## Visual Structure

### Layout Structure
Primarily vertical and stacked. The default layout follows a top-to-bottom structure with optional horizontal arrangements inside local regions such as metadata rows or action groups. Internal grouping uses existing layout container patterns.

### Content Areas or Slots
- **Media slot (optional)**: A top or side-aligned region for imagery, illustration, thumbnail content, or a decorative preview surface.
- **Header slot (required)**: The primary identification area containing the card title and optional status or category marker.
- **Body slot (required)**: Supporting description, summary content, or preview information that explains the card's purpose.
- **Metadata slot (optional)**: Compact supporting details such as date, author, tags, counts, or status text.
- **Action slot (optional)**: A region for one primary action and, when needed, one or more secondary actions or links.

### Alignment and Spacing Rules
- Content is aligned to a consistent internal grid with shared left and right padding.
- Vertical spacing separates regions clearly: header to body is tighter than body to action area.
- Metadata rows and action rows may use horizontal layout containers with consistent gaps between items.
- If media is present, its relationship to the content block should feel intentional: edge-to-edge within the card frame or inset as part of the card padding system.
- Text aligns to the same primary content edge even when badges, icons, or actions are present.
- Footer actions align as a group and do not visually compete with the title or summary content.

### Subcomponents
- `Layout-Containers`: Used to organize the card into stacked, row, grid, and section-like internal regions.
- `Button`: Used for primary or secondary actions in the footer or action row.
- `Heading`: Used for the main title and optional section labels inside the card.
- `Text`: Used for supporting copy, labels, and small metadata strings.
- `Link`: Used when the card exposes secondary navigation or inline destinations.
- `Badge`: Used for optional status, category, or priority markers.
- `Image`: Used for optional preview imagery, illustration, or thumbnail content.

---

## Variants
- **Informational**: Used for summaries, read-only previews, and modular content blocks where scanning matters more than immediate action. Uses restrained emphasis and balanced spacing.
- **Interactive**: Used when the card itself is clickable or acts as a prominent navigation target. The whole surface communicates affordance while preserving clear internal hierarchy.
- **Feature**: Used for highlighted content, promotions, or important surfaced content. May use stronger media presence, larger spacing, or more prominent headline treatment.
- **Compact**: Used in dense lists or dashboards where the same card pattern must repeat in limited space. Reduces padding and truncates secondary detail before reducing title clarity.
- **Status**: Used when the card needs to foreground a badge, state, or workflow condition such as success, warning, draft, or blocked.

---

## States
### Default
The card surface is visible and stable, with all regions rendered at full opacity. The hierarchy is immediately legible, with the title as the dominant entry point and supporting content arranged beneath it.

### Hover
For interactive cards, the surface may lift slightly through shadow, border contrast, or background shift. Hover feedback applies to the container without causing layout shift. Non-interactive cards do not imply clickability.

### Active
Interactive cards may show a pressed state through reduced elevation, subtle scale change, or darker surface treatment. Internal buttons or links retain independent active states and do not visually conflict with the card's press feedback.

### Disabled
The card appears unavailable through reduced contrast and suppressed affordance. Interactive behaviors are blocked. Internal actions inside a disabled card are also unavailable or omitted.

### Loading
Loading cards preserve the final layout footprint to avoid reflow in lists or grids. Placeholder regions stand in for media, title, text, metadata, and actions. The overall structure remains recognizable even while content is not yet available.

---

## Properties
- `title`: String. Required. The primary heading for the card.
- `description`: String or rich text summary. Optional but expected in most informational uses.
- `variant`: `informational` | `feature` | `compact` | `status` | `interactive`.
- `media`: Optional media content, image reference, or illustrative surface.
- `badge`: Optional status, category, or emphasis label.
- `metadata`: Optional list or grouped set of supporting details.
- `actions`: Optional action set, typically one primary action plus secondary actions or links.
- `orientation`: `vertical` | `horizontal`. Controls whether media and content stack or sit side by side.
- `interactive`: Boolean. When `true`, the whole card behaves as a target and exposes hover, active, and focus treatment.
- `selected`: Boolean. Optional. Indicates the card is currently chosen within a set.
- `disabled`: Boolean. Optional. Prevents interaction and applies disabled styling.
- `loading`: Boolean. Optional. Replaces content with loading placeholders while preserving size and structure.

---

## Implementation Contract

### Props
- `title`: required=`true`; shape=`scalar`; type=`String`
- `description`: required=`false`; shape=`scalar`; type=`String`
- `variant`: required=`false`; shape=`enum`; type=`String`; allowed=`informational|interactive|feature|compact|status`
- `media`: required=`false`; shape=`scalar`; type=`MediaReference`
- `badge`: required=`false`; shape=`scalar`; type=`Badge`
- `metadata`: required=`false`; shape=`list`; type=`MetadataItem`; item_contract=`MetadataItem`
- `actions`: required=`false`; shape=`list`; type=`ActionItem`; item_contract=`ActionItem`
- `orientation`: required=`false`; shape=`enum`; type=`String`; allowed=`vertical|horizontal`
- `interactive`: required=`false`; shape=`scalar`; type=`Boolean`
- `selected`: required=`false`; shape=`scalar`; type=`Boolean`
- `disabled`: required=`false`; shape=`scalar`; type=`Boolean`
- `loading`: required=`false`; shape=`scalar`; type=`Boolean`

### Object Contracts
#### `MetadataItem`
- `label`: required=`true`; shape=`scalar`; type=`String`
- `value`: required=`true`; shape=`scalar`; type=`String`
- `icon`: required=`false`; shape=`scalar`; type=`IconReference`

#### `ActionItem`
- `label`: required=`true`; shape=`scalar`; type=`String`
- `action`: required=`true`; shape=`scalar`; type=`ActionReference`
- `variant`: required=`false`; shape=`enum`; type=`String`; allowed=`primary|secondary|outlined|ghost|destructive`
- `icon`: required=`false`; shape=`scalar`; type=`IconReference`

### Collection Contracts
- `metadata`: item_contract=`MetadataItem`; behavior=`repeated-item`
- `actions`: item_contract=`ActionItem`; behavior=`repeated-item`

### Interaction Contracts
- `interactive`: kind=`navigational`
  Applies when `interactive` is `true`.
- `actions[*].action`: kind=`callback-driven`

### Composition Contracts
- `Layout-Containers`: usage=`required`
- `Button`: usage=`required`
- `Heading`: usage=`required`
- `Text`: usage=`required`
- `Link`: usage=`optional`
- `Badge`: usage=`optional`
- `Image`: usage=`optional`

### Brand Constraints
- `typography`: Typography must use `Inter` as the primary typeface for all visible text in this component, aligning with the brand's emphasis on clarity and readability.
- `color`: Color must use `brand.colors.primary.red` for emphasis in the **Feature** variant to introduce energy and visual prominence.
- `color`: Color must use `brand.colors.primary.white` for the **Informational** and **Compact** variants' background to maintain a clean, neutral foundation.
- `color`: Color must use `brand.colors.semantic.green` for the **Status** variant when indicating success or positive states.
- `color`: Color must use `brand.colors.semantic.blue` for the **Status** variant when indicating cautionary or informational states.
- `hierarchy`: Hierarchy must ensure the **title** remains the dominant entry point in all variants, with supporting content (e.g., metadata, actions) visually subordinated.
- `spacing`: Spacing must preserve generous whitespace between regions (e.g., header, body, actions) to support the brand's low-clutter, balanced compositions.

---

## Accessibility Notes
### Keyboard Interaction Expectations
- If the whole card is interactive, it must be reachable via `Tab` as a single focus target unless the pattern intentionally exposes multiple internal controls.
- `Enter` and `Space` activate the card when it behaves like a button-like surface.
- Internal buttons and links must remain reachable in a predictable focus order when present.
- If both the card surface and internal actions are interactive, focus behavior must avoid duplicate or confusing activation paths.

### ARIA Roles and Accessibility Considerations
- Non-interactive cards should render as semantic grouping content such as `<section>`, `<article>`, or `<div>` depending on context.
- Interactive cards should use the semantic element that best matches behavior, such as a link for navigation or a button-like pattern for in-place actions.
- The card must expose a clear accessible name, typically derived from the `title`.
- Status badges, counts, and metadata must be announced in a meaningful order and must not replace the `title` as the primary accessible identifier.
- Loading cards must communicate busy or updating status when the card updates asynchronously.

---

## HTML Mappings
⚠️ Fallback Applied: Inferred HTML element as `<article>` for the **Card** component when used as a standalone content block.
⚠️ Fallback Applied: Inferred HTML element as `<div>` for the **Card** component when used as a generic container.
⚠️ Fallback Applied: Inferred HTML element as `<a>` for the **Card** component when `interactive` is `true` and the card behaves as a navigational link.
⚠️ Fallback Applied: Inferred HTML element as `<button>` for the **Card** component when `interactive` is `true` and the card behaves as an action trigger.


number 2

# Card

## Component Metadata
- **Name**: Card
- **Description**: A Card is a composed surface component used to group related content, status, and actions into a single readable unit. It is used for previews, summaries, feature highlights, and modular content blocks where a clear boundary helps scanning and comparison. This component relies on existing library components for its internal building blocks.

---

## Visual Structure

### Layout Structure
- Primarily vertical and stacked, following a top-to-bottom structure.
- Optional horizontal arrangements within local regions such as metadata rows or action groups.
- Internal grouping uses existing layout container patterns.

### Content Areas or Slots
- **Media slot (optional)**: A top or side-aligned region for imagery, illustration, thumbnail content, or a decorative preview surface.
- **Header slot (required)**: The primary identification area containing the card title and optional status or category marker.
- **Body slot (required)**: Supporting description, summary content, or preview information that explains the card's purpose.
- **Metadata slot (optional)**: Compact supporting details such as date, author, tags, counts, or status text.
- **Action slot (optional)**: A region for one primary action and, when needed, one or more secondary actions or links.

### Alignment and Spacing Rules
- Content is aligned to a consistent internal grid with shared left and right padding.
- Vertical spacing separates regions clearly: header to body is tighter than body to action area.
- Metadata rows and action rows may use horizontal layout containers with consistent gaps between items.
- If media is present, its relationship to the content block should feel intentional: edge-to-edge within the card frame or inset as part of the card padding system.
- Text aligns to the same primary content edge even when badges, icons, or actions are present.
- Footer actions align as a group and do not visually compete with the title or summary content.

### Subcomponents
- `Layout-Containers`: Used to organize the card into stacked, row, grid, and section-like internal regions.
- `Button`: Used for primary or secondary actions in the footer or action row.
- `Heading`: Used for the main title and optional section labels inside the card.
- `Text`: Used for supporting copy, labels, and small metadata strings.
- `Link`: Used when the card exposes secondary navigation or inline destinations.
- `Badge`: Used for optional status, category, or priority markers.
- `Image`: Used for optional preview imagery, illustration, or thumbnail content.

---

## Variants
- **Informational**: Used for summaries, read-only previews, and modular content blocks where scanning matters more than immediate action. Uses restrained emphasis and balanced spacing.
- **Interactive**: Used when the card itself is clickable or acts as a prominent navigation target. The whole surface communicates affordance while preserving clear internal hierarchy.
- **Feature**: Used for highlighted content, promotions, or important surfaced content. May use stronger media presence, larger spacing, or more prominent headline treatment.
- **Compact**: Used in dense lists or dashboards where the same card pattern must repeat in limited space. Reduces padding and truncates secondary detail before reducing title clarity.
- **Status**: Used when the card needs to foreground a badge, state, or workflow condition such as success, warning, draft, or blocked.

---

## States
### Default
The card surface is visible and stable, with all regions rendered at full opacity. The hierarchy is immediately legible, with the title as the dominant entry point and supporting content arranged beneath it.

### Hover
For interactive cards, the surface may lift slightly through shadow, border contrast, or background shift. Hover feedback applies to the container without causing layout shift. Non-interactive cards do not imply clickability.

### Active
Interactive cards may show a pressed state through reduced elevation, subtle scale change, or darker surface treatment. Internal buttons or links retain independent active states and do not visually conflict with the card's press feedback.

### Disabled
The card appears unavailable through reduced contrast and suppressed affordance. Interactive behaviors are blocked. Internal actions inside a disabled card are also unavailable or omitted.

### Loading
Loading cards preserve the final layout footprint to avoid reflow in lists or grids. Placeholder regions stand in for media, title, text, metadata, and actions. The overall structure remains recognizable even while content is not yet available.

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
- `interactive`: Boolean. When `true`, the whole card behaves as a target and exposes hover, active, and focus treatment.
- `selected`: Boolean. Optional. Indicates the card is currently chosen within a set.
- `disabled`: Boolean. Optional. Prevents interaction and applies disabled styling.
- `loading`: Boolean. Optional. Replaces content with loading placeholders while preserving size and structure.

---

## Implementation Contract

### Props
- `title`: required=`true`; shape=`scalar`; type=`String`
- `description`: required=`false`; shape=`scalar`; type=`String`
- `variant`: required=`false`; shape=`enum`; type=`String`; allowed=`informational|interactive|feature|compact|status`
- `media`: required=`false`; shape=`scalar`; type=`MediaReference`
- `badge`: required=`false`; shape=`scalar`; type=`Badge`
- `metadata`: required=`false`; shape=`list`; type=`MetadataItem`; item_contract=`MetadataItem`
- `actions`: required=`false`; shape=`list`; type=`ActionItem`; item_contract=`ActionItem`
- `orientation`: required=`false`; shape=`enum`; type=`String`; allowed=`vertical|horizontal`
- `interactive`: required=`false`; shape=`scalar`; type=`Boolean`
- `selected`: required=`false`; shape=`scalar`; type=`Boolean`
- `disabled`: required=`false`; shape=`scalar`; type=`Boolean`
- `loading`: required=`false`; shape=`scalar`; type=`Boolean`

### Object Contracts
#### `MetadataItem`
- `label`: required=`true`; shape=`scalar`; type=`String`
- `value`: required=`true`; shape=`scalar`; type=`String`
- `icon`: required=`false`; shape=`scalar`; type=`IconReference`

#### `ActionItem`
- `label`: required=`true`; shape=`scalar`; type=`String`
- `action`: required=`true`; shape=`scalar`; type=`ActionReference`
- `variant`: required=`false`; shape=`enum`; type=`String`; allowed=`primary|secondary|outlined|ghost|destructive`
- `icon`: required=`false`; shape=`scalar`; type=`IconReference`

### Collection Contracts
- `metadata`: item_contract=`MetadataItem`; behavior=`repeated-item`
- `actions`: item_contract=`ActionItem`; behavior=`repeated-item`

### Interaction Contracts
- `interactive`: kind=`navigational`
  Applies when `interactive` is `true`.
- `actions[*].action`: kind=`callback-driven`

### Composition Contracts
- `Layout-Containers`: usage=`required`
- `Button`: usage=`required`
- `Heading`: usage=`required`
- `Text`: usage=`required`
- `Link`: usage=`optional`
- `Badge`: usage=`optional`
- `Image`: usage=`optional`

### Brand Constraints
- `typography`: Typography must use `Inter` as the primary typeface for all visible text in this component, aligning with the brand's emphasis on clarity and readability.
- `color`: Color must use `brand.colors.primary.red` for emphasis in the **Feature** variant to introduce energy and visual prominence.
- `color`: Color must use `brand.colors.primary.white` for the **Informational** and **Compact** variants' background to maintain a clean, neutral foundation.
- `color`: Color must use `brand.colors.secondary.green` for the **Status** variant when indicating success or positive states.
- `color`: Color must use `brand.colors.secondary.blue` for the **Status** variant when indicating cautionary or informational states.
- `hierarchy`: Hierarchy must ensure the **title** remains the dominant entry point in all variants, with supporting content (e.g., metadata, actions) visually subordinated.
- `spacing`: Spacing must preserve generous whitespace between regions (e.g., header, body, actions) to support the brand's low-clutter, balanced compositions.

---

## Accessibility Notes

### Keyboard Interaction Expectations
- If the whole card is interactive, it must be reachable via `Tab` as a single focus target unless the pattern intentionally exposes multiple internal controls.
- `Enter` and `Space` activate the card when it behaves like a button-like surface.
- Internal buttons and links must remain reachable in a predictable focus order when present.
- If both the card surface and internal actions are interactive, focus behavior must avoid duplicate or confusing activation paths.

### ARIA Roles and Accessibility Considerations
- Non-interactive cards should render as semantic grouping content such as `<section>`, `<article>`, or `<div>` depending on context.
- Interactive cards should use the semantic element that best matches behavior, such as a link for navigation or a button-like pattern for in-place actions.
- The card must expose a clear accessible name, typically derived from the `title`.
- Status badges, counts, and metadata must be announced in a meaningful order and must not replace the `title` as the primary accessible identifier.
- Loading cards must communicate busy or updating status when the card updates asynchronously.

---

## HTML Mappings
⚠️ Fallback Applied: Inferred HTML element as `<article>` for the **Card** component when used as a standalone content block.
⚠️ Fallback Applied: Inferred HTML element as `<div>` for the **Card** component when used as a generic container.
⚠️ Fallback Applied: Inferred HTML element as `<a>` for the **Card** component when `interactive` is `true` and the card behaves as a navigational link.
⚠️ Fallback Applied: Inferred HTML element as `<button>` for the **Card** component when `interactive` is `true` and the card behaves as an action trigger.

number 3


# Card

## Component Metadata
- **Name**: Card
- **Description**: A Card is a composed surface component used to group related content, status, and actions into a single readable unit. It should be used for previews, summaries, feature highlights, and modular content blocks where a clear boundary helps scanning and comparison. This component relies on existing library components for its internal building blocks.

---

## Visual Structure

### Layout Structure
- Primarily vertical and stacked, following a top-to-bottom structure.
- Optional horizontal arrangements within local regions such as metadata rows or action groups.
- Internal grouping uses existing layout container patterns.

### Subcomponents
- `Layout-Containers`: Used to organize the card into stacked, row, grid, and section-like internal regions.
- `Button`: Used for primary or secondary actions in the footer or action row.
- `Heading`: Used for the main title and optional section labels inside the card.
- `Text`: Used for supporting copy, labels, and small metadata strings.
- `Link`: Used when the card exposes secondary navigation or inline destinations.
- `Badge`: Used for optional status, category, or priority markers.
- `Image`: Used for optional preview imagery, illustration, or thumbnail content.

### Content Areas or Slots
- **Media slot (optional)**: A top or side-aligned region for imagery, illustration, thumbnail content, or a decorative preview surface.
- **Header slot (required)**: The primary identification area containing the card title and optional status or category marker.
- **Body slot (required)**: Supporting description, summary content, or preview information that explains the card's purpose.
- **Metadata slot (optional)**: Compact supporting details such as date, author, tags, counts, or status text.
- **Action slot (optional)**: A region for one primary action and, when needed, one or more secondary actions or links.

### Alignment and Spacing Rules
- Content is aligned to a consistent internal grid with shared left and right padding.
- Vertical spacing separates regions clearly: header to body is tighter than body to action area.
- Metadata rows and action rows may use horizontal layout containers with consistent gaps between items.
- If media is present, its relationship to the content block should feel intentional: edge-to-edge within the card frame or inset as part of the card padding system.
- Text aligns to the same primary content edge even when badges, icons, or actions are present.
- Footer actions align as a group and do not visually compete with the title or summary content.

---

## Variants
- **Informational**: Used for summaries, read-only previews, and modular content blocks where scanning matters more than immediate action. Uses restrained emphasis and balanced spacing.
- **Interactive**: Used when the card itself is clickable or acts as a prominent navigation target. The whole surface communicates affordance while preserving clear internal hierarchy.
- **Feature**: Used for highlighted content, promotions, or important surfaced content. May use stronger media presence, larger spacing, or more prominent headline treatment.
- **Compact**: Used in dense lists or dashboards where the same card pattern must repeat in limited space. Reduces padding and truncates secondary detail before reducing title clarity.
- **Status**: Used when the card needs to foreground a badge, state, or workflow condition such as success, warning, draft, or blocked.

---

## States
### Default
The card surface is visible and stable, with all regions rendered at full opacity. The hierarchy is immediately legible, with the title as the dominant entry point and supporting content arranged beneath it.

### Hover
For interactive cards, the surface may lift slightly through shadow, border contrast, or background shift. Hover feedback applies to the container without causing layout shift. Non-interactive cards do not imply clickability.

### Active
Interactive cards may show a pressed state through reduced elevation, subtle scale change, or darker surface treatment. Internal buttons or links retain independent active states and do not visually conflict with the card's press feedback.

### Disabled
The card appears unavailable through reduced contrast and suppressed affordance. Interactive behaviors are blocked. Internal actions inside a disabled card are also unavailable or omitted.

### Loading
Loading cards preserve the final layout footprint to avoid reflow in lists or grids. Placeholder regions stand in for media, title, text, metadata, and actions. The overall structure remains recognizable even while content is not yet available.

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
- `interactive`: Boolean. When `true`, the whole card behaves as a target and exposes hover, active, and focus treatment.
- `selected`: Boolean. Optional. Indicates the card is currently chosen within a set.
- `disabled`: Boolean. Optional. Prevents interaction and applies disabled styling.
- `loading`: Boolean. Optional. Replaces content with loading placeholders while preserving size and structure.

---

## Implementation Contract

### Props
- `title`: required=`true`; shape=`scalar`; type=`String`
- `description`: required=`false`; shape=`scalar`; type=`String`
- `variant`: required=`false`; shape=`enum`; type=`String`; allowed=`informational|interactive|feature|compact|status`
- `media`: required=`false`; shape=`scalar`; type=`MediaReference`
- `badge`: required=`false`; shape=`scalar`; type=`Badge`
- `metadata`: required=`false`; shape=`list`; type=`MetadataItem`; item_contract=`MetadataItem`
- `actions`: required=`false`; shape=`list`; type=`ActionItem`; item_contract=`ActionItem`
- `orientation`: required=`false`; shape=`enum`; type=`String`; allowed=`vertical|horizontal`
- `interactive`: required=`false`; shape=`scalar`; type=`Boolean`
- `selected`: required=`false`; shape=`scalar`; type=`Boolean`
- `disabled`: required=`false`; shape=`scalar`; type=`Boolean`
- `loading`: required=`false`; shape=`scalar`; type=`Boolean`

### Object Contracts
#### `MetadataItem`
- `label`: required=`true`; shape=`scalar`; type=`String`
- `value`: required=`true`; shape=`scalar`; type=`String`
- `icon`: required=`false`; shape=`scalar`; type=`IconReference`

#### `ActionItem`
- `label`: required=`true`; shape=`scalar`; type=`String`
- `action`: required=`true`; shape=`scalar`; type=`ActionReference`
- `variant`: required=`false`; shape=`enum`; type=`String`; allowed=`primary|secondary|outlined|ghost|destructive`
- `icon`: required=`false`; shape=`scalar`; type=`IconReference`

### Collection Contracts
- `metadata`: item_contract=`MetadataItem`; behavior=`repeated-item`
- `actions`: item_contract=`ActionItem`; behavior=`repeated-item`

### Interaction Contracts
- `interactive`: kind=`navigational`
  Applies when `interactive` is `true`.
- `actions[*].action`: kind=`callback-driven`

### Composition Contracts
- `Layout-Containers`: usage=`required`
- `Button`: usage=`required`
- `Heading`: usage=`required`
- `Text`: usage=`required`
- `Link`: usage=`optional`
- `Badge`: usage=`optional`
- `Image`: usage=`optional`

### Brand Constraints
- `typography`: Typography must use `brand.typography.family.primary` (Inter) for all visible text in this component, aligning with the brand's emphasis on clarity and readability.
- `color`: Color must use `brand.colors.primary.red` for emphasis in the **Feature** variant to introduce energy and visual prominence.
- `color`: Color must use `brand.colors.primary.white` for the **Informational** and **Compact** variants' background to maintain a clean, neutral foundation.
- `color`: Color must use `brand.colors.semantic.green` for the **Status** variant when indicating success or positive states.
- `color`: Color must use `brand.colors.semantic.blue` for the **Status** variant when indicating cautionary or informational states.
- `hierarchy`: Hierarchy must ensure the **title** remains the dominant entry point in all variants, with supporting content (e.g., metadata, actions) visually subordinated.
- `spacing`: Spacing must preserve generous whitespace between regions (e.g., header, body, actions) to support the brand's low-clutter, balanced compositions.

---

## Accessibility Notes

### Keyboard Interaction Expectations
- If the whole card is interactive, it must be reachable via `Tab` as a single focus target unless the pattern intentionally exposes multiple internal controls.
- `Enter` and `Space` activate the card when it behaves like a button-like surface.
- Internal buttons and links must remain reachable in a predictable focus order when present.
- If both the card surface and internal actions are interactive, focus behavior must avoid duplicate or confusing activation paths.

### ARIA Roles and Accessibility Considerations
- Non-interactive cards should render as semantic grouping content such as `<section>`, `<article>`, or `<div>` depending on context.
- Interactive cards should use the semantic element that best matches behavior, such as a link for navigation or a button-like pattern for in-place actions.
- The card must expose a clear accessible name, typically derived from the `title`.
- Status badges, counts, and metadata must be announced in a meaningful order and must not replace the `title` as the primary accessible identifier.
- Loading cards must communicate busy or updating status when the card updates asynchronously.

---

## HTML Mappings
⚠️ Fallback Applied: Inferred HTML element as `<article>` for the **Card** component when used as a standalone content block.
⚠️ Fallback Applied: Inferred HTML element as `<div>` for the **Card** component when used as a generic container.
⚠️ Fallback Applied: Inferred HTML element as `<a>` for the **Card** component when `interactive` is `true` and the card behaves as a navigational link.
⚠️ Fallback Applied: Inferred HTML element as `<button>` for the **Card** component when `interactive` is `true` and the card behaves as an action trigger.