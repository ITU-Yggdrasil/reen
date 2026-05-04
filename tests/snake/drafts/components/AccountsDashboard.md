# AccountsDashboard - Component Specification

## Component Metadata

### Name

AccountsDashboard

### Description

AccountsDashboard is the primary authenticated overview surface for the Lupa account experience. It should be used to present a high-level financial summary, a list of visible accounts, and an adjacent assistant workspace in one calm, clearly structured page shell.

---

## Visual Structure

AccountsDashboard is a two-column dashboard layout with a dominant account-overview region on the left and a supporting chat workspace on the right. The page should lead with a greeting and total-balance summary before exposing repeated account cards beneath it.

### Layout Structure

Split layout on desktop with a wider left content column and a narrower right support column. On smaller screens, the layout should stack in reading order: summary first, account list second, chat panel third.

### Subcomponents

- `Layout-Containers`: used to define the page shell, split columns, stacks, and responsive grouping.
- `BalanceSummary`: used for the greeting and total-balance presentation.
- `AccountCard`: used for each visible account summary tile.
- `ChatPanel`: used for the assistant conversation area.
- `Heading`: used for major section titles and the greeting line.
- `Text`: used for descriptive, supporting, and microcopy content.

### Content Areas or Slots

- **Summary slot (required):** Greeting and total-balance region placed at the top of the left column.
- **Accounts slot (required):** Repeated account-card region showing the visible account set.
- **Assistant slot (required):** A persistent chat panel used for questions and transfer-oriented support.

### Alignment and Spacing Rules

- The summary region should visually dominate the left column before the repeated account cards begin.
- Account cards should align to a clean shared row or responsive wrapping grid.
- The right-side chat panel should feel structurally equal to the left column but visually secondary to the balance summary.
- Spacing between major regions should feel generous and stable, with enough empty space to preserve a premium, low-stress reading rhythm.
- The split layout should not feel cramped; the assistant panel must retain a usable input area even when the page narrows.

---

## Variants

- **Default:** Standard authenticated dashboard with summary, accounts, and assistant panel.
- **Compact:** Reduced spacing and tighter grouping for constrained viewport or embedded layouts.
- **Empty Accounts:** A state-ready layout that preserves structure even when no accounts are visible yet.

---

## States

### Default

The page shows greeting, total balance, visible account cards, and a ready assistant panel.

### Loading

Summary values, account cards, and assistant content may show placeholders while preserving the overall dashboard structure.

### Empty Accounts

The summary remains visible while the account list region communicates that no accounts are currently available.

### Narrow / Stacked

The split layout collapses into a clear vertical flow without losing hierarchy or readability.

---

## Properties

- `greeting`: String. Required. The primary welcome line shown above the dashboard content.
- `total_balance_label`: String. Required. The label describing the primary balance figure.
- `total_balance_value`: String. Required. The displayed total balance amount.
- `currency_code`: String. Required. The currency shown with the total balance.
- `accounts`: List. Required. The visible account summaries rendered as `AccountCard` items.
- `chat_panel`: Object. Required. Configuration for the embedded `ChatPanel`.
- `variant`: `default` | `compact` | `empty-accounts`.
- `loading`: Boolean. Optional. Indicates whether dashboard content is still loading.

---

## Accessibility Notes

### Keyboard Interaction Expectations

- Focus should move through the page in a predictable top-to-bottom, left-to-right order.
- Account cards with internal actions or navigation must remain keyboard reachable without ambiguity.
- The chat input and send action must be easy to reach after summary and account information.

### ARIA Roles and Accessibility Considerations

- The page should use semantic regions or sections to separate summary content, account content, and assistant content.
- The greeting should be exposed as a meaningful heading near the top of the main content.
- Balance values and account labels should be presented in readable text order so screen-reader users receive the same overview hierarchy as sighted users.

---

## Usage Guidelines

### Do

- Make the greeting and total balance the primary entry point for the dashboard.
- Keep the account list easy to scan by using repeated cards with consistent structure.
- Treat the assistant panel as a supportive workspace, not the dominant visual focus.

### Don't

- Don't bury the balance summary beneath repeated account cards.
- Don't overload the dashboard with dense secondary modules not visible in the intended page.
- Don't let the chat panel visually overpower the core account overview.
