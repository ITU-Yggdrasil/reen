# LoginScreen - Component Specification

## Component Metadata

### Name

LoginScreen

### Description

LoginScreen is the primary authentication landing surface for the Lupa account experience. It should be used to introduce the product in plain language, explain the value of signing in, and present one dominant identity-provider action without distracting secondary content.

---

## Visual Structure

LoginScreen is a centered hero-style authentication layout with a lightweight decorative background, a compact brand cue, a strong statement headline, supporting body copy, and one primary call to action.

### Layout Structure

Primarily vertical and centered within the available page height. The content column should read from small brand cue to statement headline to supporting description to primary action. Decorative background treatment may sit behind the content but should not interrupt readability.

### Subcomponents

- `Layout-Containers`: used to create the centered column, page spacing, and hero shell.
- `Heading`: used for the primary product statement.
- `Text`: used for the supporting sign-in explanation.
- `Button`: used for the primary authentication action.
- `Link`: used when the primary action navigates to an external or OAuth flow.

### Content Areas or Slots

- **Background accent slot (optional):** Decorative non-essential visual treatment behind the hero content.
- **Brand cue slot (required):** Short wordmark or identifier above the statement.
- **Statement slot (required):** The main value proposition or page title.
- **Support copy slot (required):** A concise explanation of what signing in enables.
- **Primary action slot (required):** One prominent sign-in action.

### Alignment and Spacing Rules

- The content column should be visually centered and easy to scan in one pass.
- The headline should carry the strongest emphasis, followed by the CTA, then the support copy.
- Copy width should stay comfortably readable and not become too wide on desktop.
- The primary action should feel clearly connected to the explanatory copy and remain close to it.
- Decorative background treatment should support atmosphere without reducing contrast.

---

## Variants

- **Default:** Standard sign-in hero with one provider-based primary action.
- **Trust-Focused:** Adds stronger reassurance messaging or security language when login trust needs extra emphasis.
- **Minimal:** Strips back decorative treatment for constrained or embedded authentication flows.

---

## States

### Default

The sign-in message and action are fully visible with the primary button ready for interaction.

### Loading

The primary action shows in-progress feedback while the authentication handoff begins, without shifting layout.

### Error (if applicable)

If sign-in cannot start, the layout remains stable and a concise error message may appear near the action area.

### Reduced-Motion Friendly

Any decorative transitions or view changes should remain subtle or be removed for reduced-motion users.

---

## Properties

- `brand_label`: String. Required. Short brand or product cue shown above the hero statement.
- `headline`: String. Required. Primary statement that introduces the experience.
- `description`: String. Required. Supporting copy explaining what sign-in provides.
- `primary_action_label`: String. Required. Text shown on the main sign-in action.
- `primary_action_href`: String. Optional. Navigation destination when the action starts an external sign-in flow.
- `provider_name`: String. Optional. Identity provider label used in CTA copy.
- `variant`: `default` | `trust-focused` | `minimal`.
- `background_accent`: Boolean. Optional. Whether to render a decorative hero background.
- `loading`: Boolean. Optional. Indicates the authentication handoff is in progress.

---

## Accessibility Notes

### Keyboard Interaction Expectations

- The primary action must be reachable quickly without tabbing through unrelated content.
- Focus order should move naturally through the hero content and then to the primary action.
- Loading or error feedback must not trap focus unexpectedly.

### ARIA Roles and Accessibility Considerations

- The hero should be structured with a clear heading hierarchy centered on the main statement.
- Supporting copy should remain semantic paragraph text and not be broken into decorative fragments.
- If the primary CTA renders as a link styled like a button, the accessible name must still describe the authentication action clearly.

---

## Usage Guidelines

### Do

- Keep the page purpose obvious within the first few seconds of reading.
- Use one dominant sign-in action when the authentication path is straightforward.
- Write support copy that explains the benefit of signing in in plain, confidence-building language.

### Don't

- Don't dilute the page with multiple competing calls to action.
- Don't place dense legal or product explanation inside the hero itself.
- Don't let decorative background effects compete with the headline or button.
