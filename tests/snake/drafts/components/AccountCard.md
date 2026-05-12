# AccountCard - Component Specification

## Component Metadata

### Name

AccountCard

### Description

AccountCard is the repeated account summary tile used to present one account holder label, a masked account number, and the current account balance. It should be used in collections where users need to scan multiple accounts quickly without entering a detailed ledger view.

---

## Visual Structure

AccountCard is a bounded, horizontally oriented information card with a small leading account icon area and a right-side content stack containing owner name, masked account number, and balance.

### Layout Structure

Two-zone layout with a compact leading visual marker and a larger content area. The content area itself stacks identification details above the balance figure.

### Subcomponents

- `Card`: used as the structural surface for the tile.
- `Icon`: used for the leading account indicator.
- `Text`: used for the masked account number and currency marker.
- `Heading`: used for the account holder or account title.
- `Badge`: optional, used when account state or type needs a visible marker.

### Content Areas or Slots

- **Icon slot (required):** Leading visual marker for the account type or banking product.
- **Title slot (required):** The visible account owner or account name.
- **Identifier slot (required):** The masked account number or short account reference.
- **Balance slot (required):** The account balance.
- **Currency slot (required):** The currency code paired with the balance.

### Alignment and Spacing Rules

- The icon should remain visually secondary to the account title and balance.
- Title and masked number should form one readable identification cluster.
- The balance should sit lower in the card and remain easy to scan across repeated cards.
- Repeated cards in the same row should share consistent padding, height behavior, and spacing.

---

## Variants

- **Default:** Standard account tile used in the main dashboard overview.
- **Positive Balance:** A tone-aware presentation when the account value is positive.
- **Negative Balance:** A tone-aware presentation when the account value is below zero.
- **Selected:** Used when one account is currently active in a broader interaction.

---

## States

### Default

All account details are visible and the card acts as a stable summary tile.

### Hover

If interactive, the card may gain subtle emphasis without shifting layout.

### Selected

The card shows clear but restrained active-state treatment.

### Loading

The card preserves its structure while values or identifiers are still loading.

---

## Properties

- `title`: String. Required. The visible account owner or account label.
- `masked_identifier`: String. Required. The masked account number or short account reference.
- `balance_value`: String. Required. The displayed account balance.
- `currency_code`: String. Required. The currency paired with the balance.
- `icon`: Icon reference. Optional. The account-type marker.
- `badge`: Object. Optional. Status or type marker for the account.
- `variant`: `default` | `positive-balance` | `negative-balance` | `selected`.
- `interactive`: Boolean. Optional. Whether the card behaves as a selectable target.
- `loading`: Boolean. Optional. Indicates that account information is still resolving.

---

## Accessibility Notes

### Keyboard Interaction Expectations

- If the card is interactive, it must be reachable as a clear focus target.
- If the card is not interactive, it should not be focusable solely for presentation.

### ARIA Roles and Accessibility Considerations

- The title should provide the primary accessible identifier for the account tile.
- Masked account identifiers should remain understandable when read aloud.
- Balance value and currency should be announced together in a meaningful order.

---

## Usage Guidelines

### Do

- Use repeated cards with identical structure to make comparison easy.
- Keep the balance and account title visually clear at a glance.
- Use masked identifiers consistently across all tiles.

### Don't

- Don't overcrowd the card with transaction-level detail.
- Don't make decorative iconography more prominent than the balance.
- Don't vary spacing or hierarchy from one card to the next in the same collection.
