# Form

## Description
Form is the composed input workflow component for collecting, validating, and submitting related user data. Use it when multiple fields belong to a shared task or decision, and avoid using a full form wrapper when a single lightweight action can be handled with one isolated Input and Button.

## Purpose
The purpose of Form is to organize related inputs into a complete and understandable data-entry flow.

## Visual Structure
Form is a vertically or logically grouped set of labeled controls, supporting text, validation messaging, and submission actions. The structure should guide the user from context to entry to confirmation with clear spacing and visible relationships between fields.

## Subcomponents
- Label
- Input
- Button
- Text
- Optional helper text
- Optional validation message

## States & Variants
- States: idle, in-progress, valid, invalid, submitting, submitted, disabled.
- Variants: single-column, multi-column, inline, step-based.
- Validation patterns: immediate, on blur, on submit.

## Properties
- `title`: optional form heading.
- `fields`: documented list of form controls and roles.
- `layout`: stacking or grouping strategy.
- `requiredFields`: fields that must be completed.
- `validationMode`: when validation feedback appears.
- `submitAction`: primary action behavior.
- `secondaryAction`: optional cancel or reset behavior.
- `statusMessage`: summary feedback after validation or submission.

## Brand Reference
No formal brand specification is required for this draft. Form should feel consistent, accessible, and trustworthy through clear structure, concise labeling, and understandable feedback. Layout and state communication should reduce cognitive load without relying on a custom brand language.
