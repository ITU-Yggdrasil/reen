# Footer - Component Specification

## Component Metadata

### Name

Footer

### Description

Footer is the low-emphasis site footer used to present lightweight legal navigation, ownership labeling, and trust-supporting microcopy at the end of the page. It should be used to close the page quietly without competing with the primary dashboard content.

---

## Visual Structure

Footer is a compact horizontal information row containing copyright text, an optional trust or security badge, and a small set of legal links.

### Layout Structure

Horizontal layout on wide screens with grouped informational items that may wrap into a stacked arrangement on narrow screens. The component should feel secondary and visually restrained.

### Subcomponents

- `Layout-Containers`: used to arrange the footer into inline groups and responsive wrapping behavior.
- `Text`: used for copyright and supporting microcopy.
- `Link`: used for legal or informational destinations.
- `Icon`: used for optional trust or security indicators.

### Content Areas or Slots

- **Ownership slot (required):** Copyright or product ownership text.
- **Trust badge slot (optional):** Small icon-plus-text signal for security, compliance, or verification.
- **Legal links slot (optional but expected):** One or more low-priority links such as privacy or terms.

### Alignment and Spacing Rules

- Footer items should align to one calm baseline when space allows.
- Legal links should group together as a secondary cluster rather than feeling evenly scattered.
- Supporting badge content should remain subtle and not resemble a primary CTA.
- The footer should maintain comfortable separation from the main content above it.

---

## Variants

- **Default:** Standard informational footer with ownership text and legal links.
- **Trust-Enhanced:** Includes a compact verification or security badge.
- **Minimal:** Ownership text only for the simplest screens.

---

## States

### Default

All footer content is visible and presented with low visual weight.

### Wrapped / Narrow

Groups reflow into multiple lines or stacks while preserving readable grouping.

### Link Hover

Legal links gain recognizable interactive feedback without becoming louder than the page's primary actions.

---

## Properties

- `copyright_label`: String. Required. Ownership text shown in the footer.
- `legal_links`: List. Optional. Link items such as privacy and terms.
- `trust_badge_label`: String. Optional. Supporting security or verification label.
- `trust_badge_icon`: Icon reference. Optional. Decorative or informative icon paired with the trust label.
- `variant`: `default` | `trust-enhanced` | `minimal`.

---

## Accessibility Notes

### Keyboard Interaction Expectations

- Footer links must remain reachable after the main content in a natural tab order.
- Link focus styling should be visible even when the footer is intentionally muted.

### ARIA Roles and Accessibility Considerations

- The footer should render within a semantic `<footer>` region.
- Decorative icons used inside a trust badge should be hidden from assistive technologies unless they carry unique meaning.
- Legal links should have descriptive accessible names that match their visible text.

---

## Usage Guidelines

### Do

- Keep the footer concise and secondary.
- Use it for legal destinations and small trust cues rather than feature navigation.
- Maintain readable contrast even when the component is visually quiet.

### Don't

- Don't turn the footer into a second navigation bar.
- Don't add dense product messaging or promotional content.
- Don't give trust-badge styling so much weight that it competes with the page headline or primary account actions.
