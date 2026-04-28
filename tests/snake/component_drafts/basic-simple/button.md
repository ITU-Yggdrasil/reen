# Button

## Description
Button is the primary action component for triggering an immediate user action such as submitting, confirming, starting, saving, or dismissing. Use it for actions that change state or move a flow forward, and avoid using it for plain navigation when a Link is more appropriate.

## Purpose
The purpose of Button is to give users a clear, high-confidence way to trigger an action.

## Visual Structure
The button is a compact rectangular control with a clear label centered inside a padded click target. It may include a leading or trailing icon when the icon supports the label rather than replacing it. The silhouette should feel stable, readable, and easy to identify at a glance.

## Subcomponents
None.

## States & Variants
- States: default, hover, focus-visible, pressed, disabled, loading.
- Variants: primary, secondary, tertiary, destructive.
- Sizes: small, medium, large.
- Width behaviors: intrinsic width or full-width.

## Properties
- `label`: visible action text.
- `variant`: visual priority and emphasis level.
- `size`: control height and padding scale.
- `disabled`: blocks interaction.
- `loading`: shows in-progress feedback.
- `iconLeading`: optional icon before the label.
- `iconTrailing`: optional icon after the label.
- `fullWidth`: expands to container width when needed.
- `type`: semantic button type such as button, submit, or reset.

## Brand Reference
No formal brand specification is required for this draft. Button should follow neutral interface principles: clear labeling, simple geometry, strong contrast, accessible interactive states, and restrained decoration. Visual emphasis should come from hierarchy and usability rather than a custom brand treatment.
