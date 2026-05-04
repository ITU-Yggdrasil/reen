# BalanceSummary - Component Specification

## Component Metadata

### Name

BalanceSummary

### Description

BalanceSummary is the high-emphasis overview component used to present a greeting, a summary label, and one dominant balance figure. It should be used near the top of the accounts dashboard to orient the user quickly before they scan individual accounts.

---

## Visual Structure

BalanceSummary is a vertically stacked summary block with a prominent greeting line, a smaller uppercase-style summary label, and a large balance amount paired with a currency marker.

### Layout Structure

Vertical stack with tight grouping between the balance label and value, and slightly larger separation between the greeting and the summary values.

### Subcomponents

- `Heading`: used for the greeting line.
- `Text`: used for the balance label and currency marker.
- `Layout-Containers`: used to group the value row and preserve consistent spacing.

### Content Areas or Slots

- **Greeting slot (required):** The primary welcome line.
- **Summary label slot (required):** A short descriptor such as total balance.
- **Balance value slot (required):** The dominant numeric figure.
- **Currency slot (required):** The currency code paired with the balance value.

### Alignment and Spacing Rules

- The greeting should be the first visible element in the block.
- The balance value should carry the strongest visual emphasis.
- The currency marker should feel attached to the balance value without competing with it.
- Spacing should keep the block compact enough to feel like one unit while leaving enough air for the large figure to breathe.

---

## Variants

- **Default:** Standard dashboard summary with full greeting and total balance.
- **Compact:** Reduced spacing and smaller value treatment for constrained layouts.
- **Alerted:** Optional state where the balance treatment can reflect that the value deserves extra attention.

---

## States

### Default

The greeting, label, value, and currency are all visible at full emphasis according to their role.

### Loading

The component preserves the same footprint with placeholder text or value treatment while data resolves.

### Negative Balance

The balance may reflect a negative value while preserving readability and composure.

---

## Properties

- `greeting`: String. Required. The main welcome line.
- `label`: String. Required. The descriptor for the balance value.
- `value`: String. Required. The displayed balance amount.
- `currency_code`: String. Required. The currency paired with the value.
- `variant`: `default` | `compact` | `alerted`.
- `loading`: Boolean. Optional. Indicates that the values are not yet ready.

---

## Accessibility Notes

### Keyboard Interaction Expectations

- This component is typically read-only and should not introduce unnecessary focus targets.

### ARIA Roles and Accessibility Considerations

- The greeting should use a clear heading level appropriate for the page hierarchy.
- The label, value, and currency should be read in a meaningful order.
- Large number presentation should remain understandable when announced by assistive technologies.

---

## Usage Guidelines

### Do

- Use this component as the first information block in the dashboard.
- Preserve strong contrast and number readability.
- Keep the summary language concise and reassuring.

### Don't

- Don't overload the summary with secondary metrics.
- Don't hide the currency code or detach it visually from the value.
- Don't add decorative clutter that weakens the balance figure's clarity.
