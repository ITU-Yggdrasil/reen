# TopNav - Component Specification

## Component Metadata

### Name

TopNav

### Description

TopNav is the global header navigation component for the Lupa account experience. It should be used to present the product wordmark, a small set of primary destinations, signed-in account context, and utility controls such as specification tooling, theme switching, and sign-out without overwhelming the page chrome.

---

## Visual Structure

TopNav is a slim horizontal bar that anchors the page and establishes orientation before the main content. It contains a left-aligned brand area, a compact navigation group, and a right-aligned utility cluster that may also include current-account identity and sign-out controls.

### Layout Structure

Horizontal shell layout with three zones: identity on the left, primary navigation near the center or left-middle, and utility actions on the right. On narrow widths the component should preserve the wordmark and utility controls first, and collapse low-priority navigation before reducing core actions.

### Subcomponents

- `Layout-Containers`: used to create the overall shell, internal rows, and spacing rhythm.
- `Link`: used for the brand home link and the primary navigation destinations.
- `Button`: used for utility actions such as spec mode, spec drawer toggles, and sign-out.
- `ThemeToggle`: used as the grouped theme preference control.
- `Text`: used for wordmark-adjacent account identity and supporting utility labels.

### Content Areas or Slots

- **Brand slot (required):** A linked wordmark or brand identifier that returns the user to the home screen.
- **Primary links slot (optional but expected):** One or a few top-level destinations such as accounts, transfers, chat, or specifications.
- **Account identity slot (optional):** Signed-in customer or tenant context shown near the utility area.
- **Utility actions slot (optional but expected):** Secondary controls for environment preferences, tooling actions, and sign-out.

### Alignment and Spacing Rules

- The brand slot should remain the most visually stable element in the bar.
- Navigation links should sit on the same baseline as the utility area.
- Account identity and utility controls should align as one grouped cluster rather than feeling like unrelated floating items.
- Horizontal spacing should be generous enough to avoid crowding, but compact enough that the bar still feels lightweight.
- The component should preserve a clear reading order from brand to navigation to account context to utilities.

---

## Variants

- **Default:** Standard application header for the authenticated account experience.
- **Minimal:** Reduced navigation presence for focused surfaces such as legal pages.
- **Spec-Aware:** Includes specification mode and drawer triggers for internal review environments.

---

## States

### Default

The navigation bar is visible, stable, and fully readable with all configured controls available.

### Scrolled

The bar may become slightly more distinct from page content through surface contrast, border, or shadow when the page scrolls.

### Collapsed

Lower-priority links are reduced or moved to overflow on narrow viewports while identity and critical utilities remain accessible.

### Disabled Utility Action (if applicable)

A utility control may appear unavailable when a feature such as the spec drawer cannot be opened in the current environment.

---

## Properties

- `brand_label`: String. Required. The visible or accessible brand identifier.
- `brand_href`: String. Required. The destination for the brand link.
- `items`: List. Optional. Primary navigation entries shown in the header.
- `account_label`: String. Optional. Visible signed-in account or customer identifier.
- `utility_actions`: List. Optional. Auxiliary buttons or controls shown on the right.
- `show_spec_tools`: Boolean. Optional. Controls whether spec-mode tooling appears.
- `theme_toggle`: Object. Optional. Configuration for an embedded `ThemeToggle`.
- `sign_out_action`: Object. Optional. Sign-out control shown in the utility cluster.
- `variant`: `default` | `minimal` | `spec-aware`.
- `sticky`: Boolean. Optional. Whether the bar remains pinned at the top edge.

---

## Accessibility Notes

### Keyboard Interaction Expectations

- Interactive elements in the navigation must be reachable in a predictable left-to-right tab order.
- Toggle buttons in the utility area must expose pressed state when applicable.
- Collapsed navigation patterns must remain keyboard reachable when overflow behavior is introduced.

### ARIA Roles and Accessibility Considerations

- The component should render within a semantic `<nav>` region with an appropriate label such as `Primary`.
- The brand link must have a clear accessible name that communicates destination.
- Grouped theme or utility toggles should expose their relationship through grouping semantics when rendered as a control cluster.

---

## Usage Guidelines

### Do

- Keep the top-level navigation set intentionally short.
- Use the right-side utility cluster for preference or tooling actions that are global to the page.
- Preserve a clean, low-noise header so the main dashboard content remains the focal point.

### Don't

- Don't overload the bar with dense status messaging or dense secondary navigation.
- Don't allow utility controls to visually overpower the brand or page purpose.
- Don't hide the brand link or theme control when reducing layout width unless a stronger mobile pattern replaces them.
