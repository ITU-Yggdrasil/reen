# Page - Component Specification

## Component Metadata

### Name

Page

### Description

A Page is a top-level composed component used to assemble a complete site view from the design system's existing components. It should be used to define the full page shell, section order, and page-level hierarchy when a screen needs to contain navigation, content, actions, supporting information, and overlays in one coordinated structure.

---

## Visual Structure

The Page is a full-width, top-level composition container that organizes the site into clear regions such as global navigation, page header, main content, supporting content, and footer. It should feel like a complete page rather than a floating module, with strong hierarchy, predictable section rhythm, and enough flexibility to hold all other components without losing structure.

### Layout Structure

Primarily vertical and section-based. The default layout moves from top-level navigation into a page-introduction region and then into one or more main content sections. Local regions may switch to row, grid, or split flows by composing existing components inside Page slots, without introducing a separate layout-system component.

### Subcomponents

- `Button`: used for primary and secondary page-level actions.
- `Badge`: used for page-level status, category, or emphasis markers.
- `Image`: used for hero media, previews, illustrations, or supporting visual anchors.
- `Card`: used for grouped summaries, previews, repeated modules, or featured content blocks.

### Content Areas or Slots

Include:

- **Navbar slot (optional):** A top-level navigation region for brand identity, primary destinations, and global actions.
- **Page header slot (required):** The page introduction area containing the main heading and optional supporting copy, status markers, actions, or hero content.
- **Primary content slot (required):** The main body region where the page's core sections and content modules appear.
- **Supporting content slot (optional):** A region for related information such as side content, summaries, tables, forms, or secondary cards.
- **Footer slot (optional):** A concluding region for additional navigation, supporting links, legal copy, or low-priority actions.
- **Overlay slot (optional):** A layered region for modal content when page-level interruption is needed.

### Alignment and Spacing Rules

- The page should establish a consistent outer shell and shared horizontal rhythm across all regions.
- Section spacing should make major page transitions obvious without making the page feel fragmented.
- The page header should feel visually dominant over supporting regions and repeated modules.
- Repeated cards, forms, tables, and content groups should align to the same underlying layout system.
- Supporting content should remain clearly secondary to the primary content slot even when it is visually rich.
- Overlays such as modals should layer above the page without disrupting the readable structure beneath them.

---

## Variants

- **Landing:** Used for marketing-style or introductory pages where hero hierarchy, broad section rhythm, and strong entry messaging are the primary focus.
- **Dashboard:** Used for dense information pages that combine summaries, cards, status indicators, tables, and actions inside a stable shell.
- **Content:** Used for editorial, documentation, or reading-oriented pages where headings, paragraphs, links, and supporting imagery dominate.
- **Workflow:** Used for task-oriented pages where forms, inputs, labels, status feedback, and supporting actions lead the experience.

For each variant, clarify:

- Landing differs by making the page header and introductory content more visually prominent than utility density.
- Dashboard differs by emphasizing modular regions, repeated cards, and structured information over narrative flow.
- Content differs by prioritizing reading rhythm, section hierarchy, and supporting media over dense controls.
- Workflow differs by prioritizing guided task completion, form grouping, and action clarity.

---

## States

### Default

The full page shell is visible and stable, with navigation, hierarchy, and section structure all clearly readable.

### Loading

The page preserves its overall structure while primary regions may show placeholders, reduced-detail content, or loading surfaces so the layout does not shift unexpectedly.

### Empty

The page remains structurally complete even when a primary content region has no data, using clear messaging and actions without collapsing the whole layout.

### Error

The page continues to expose the surrounding shell and orientation while making the affected region or blocking message clear enough for recovery.

---

## Properties

- `title`: String. Required. The primary page heading.
- `variant`: `landing` | `dashboard` | `content` | `workflow`.
- `showNavbar`: Boolean. Optional. Controls whether the page includes the navbar slot.
- `showFooter`: Boolean. Optional. Controls whether the page includes the footer slot.
- `hasSupportingContent`: Boolean. Optional. Indicates whether the supporting content slot is present.
- `hasOverlay`: Boolean. Optional. Indicates whether the page may present modal content above the page shell.
- `status`: Optional short status or context label shown near the page header when relevant.
- `loading`: Boolean. Optional. Puts the page into its loading state while preserving the page shell.
- `empty`: Boolean. Optional. Indicates the primary content region has no content to show.
- `error`: Boolean. Optional. Indicates the page is showing an error state for one or more regions.

---

## Accessibility Notes

### Keyboard Interaction Expectations

- The page should preserve a logical `Tab` order from navigation into the page header and then through the primary content.
- Page-level actions, links, form controls, and modal triggers must remain reachable in a predictable order.
- When a modal is present, keyboard focus should move into the modal and return to the invoking region when the modal closes.
- Keyboard access should not depend on visual grouping alone; each interactive child component should retain its documented behavior.

### ARIA Roles and Accessibility Considerations

- The page should use meaningful structural landmarks such as `<header>`, `<nav>`, `<main>`, `<aside>`, `<footer>`, and modal semantics when those regions are present.
- The page should expose exactly one primary page heading for the main content view.
- The primary content slot should remain identifiable as the main destination for assistive technologies even when supporting content is present.
- Empty, loading, and error messaging should be announced in a way that preserves orientation and does not hide the rest of the page structure.
- Reused child components must retain their own accessible names, roles, and interaction behavior inside the page.

---

## Optional: Usage Guidelines and Examples

### Do

- Use Page when the design needs one top-level shell that coordinates all other components into a complete site view.
- Reuse existing components for all local structure and interaction instead of redefining bespoke page-only children.
- Keep one clear page heading and a readable order of sections so the page remains easy to scan.
- Choose a variant based on the dominant page purpose rather than mixing unrelated page patterns into one shell.

### Don't

- Don't treat Page as a one-off mockup tied to a single screen's content.
- Don't bypass existing components by embedding custom child behavior directly into the page definition.
- Don't let supporting content or overlays overwhelm the primary content hierarchy.
- Don't duplicate navigation, action, or messaging patterns that are already defined by child components.

---

## Notes

- This document defines **one component specification** and should stay focused on that component only.
- Page is a reusable full-page composition component, not a brand-specific mockup or a replacement for the child component library.
- Internal child behavior should defer to existing component drafts where those components are already defined in the library.
- Prioritize hierarchy, composition, and reuse across variants, states, and accessibility behavior.