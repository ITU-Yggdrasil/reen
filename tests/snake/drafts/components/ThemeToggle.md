# ThemeToggle - Component Specification

## Component Metadata

### Name

ThemeToggle

### Description

ThemeToggle is a compact grouped control used to switch between light, dark, and system theme preferences. It should be used anywhere the interface needs a persistent display-mode preference without opening a separate settings screen.

---

## Visual Structure

ThemeToggle is a small segmented control composed of adjacent options with a shared boundary and one visibly active choice.

### Layout Structure

Horizontal inline group with evenly spaced toggle buttons arranged inside one bounded control.

### Subcomponents

- `Layout-Containers`: used to group options into one compact control.
- `Button`: used for each selectable theme option.
- `Text`: used for short option labels where icon-only treatment is not appropriate.

### Content Areas or Slots

- **Choice group (required):** The set of theme options available to the user.
- **Selected state indicator (required):** Visual emphasis showing which option is currently active.

### Alignment and Spacing Rules

- All choices should share the same height and feel like part of one control rather than separate standalone buttons.
- The active option should be immediately recognizable without relying on color alone.
- Labels should remain short so the control stays compact within header layouts.

---

## Variants

- **Text Segmented:** Uses short text labels such as Light, Dark, and System.
- **Icon Assisted:** Uses icons with optional text when additional visual recognition is useful.
- **Compact:** Reduced spacing for tight header utility regions.

---

## States

### Default

All choices are visible and one option is selected.

### Hover

The hovered option gains light emphasis without obscuring which option is currently active.

### Active / Pressed

The selected option appears clearly engaged and updates immediately after user action.

### Disabled

The entire control or an individual option may appear unavailable and should not accept interaction.

---

## Properties

- `options`: List. Required. Available theme choices, typically light, dark, and system.
- `selected`: String. Required. The active theme choice.
- `variant`: `text-segmented` | `icon-assisted` | `compact`.
- `disabled`: Boolean. Optional. Prevents theme switching.
- `aria_label`: String. Optional. Accessible label for the control group when no visible label is present.

---

## Accessibility Notes

### Keyboard Interaction Expectations

- Users must be able to reach the control and switch options using keyboard interaction.
- If implemented as a grouped toggle pattern, arrow-key navigation may be used in addition to `Tab`.
- The selected option must be perceivable to assistive technologies.

### ARIA Roles and Accessibility Considerations

- The group should expose a clear label such as `Theme`.
- Each option should expose pressed or selected state consistently.
- The control should avoid relying on color alone to distinguish the active choice.

---

## Usage Guidelines

### Do

- Use concise labels and a consistent option order.
- Keep the toggle visually quiet enough for header placement while still discoverable.
- Preserve the user’s last selection when the broader application supports saved preferences.

### Don't

- Don't mix unrelated display preferences into this control.
- Don't make the active state ambiguous when hover or focus is also present.
- Don't use long labels that force the control to dominate the header.
