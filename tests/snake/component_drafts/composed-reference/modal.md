# Modal

## Description
Modal is an overlay component used to interrupt the current flow for focused decisions, confirmations, or contained tasks. Use it when temporary focus and explicit acknowledgment are necessary, and avoid using it for information that could be shown inline without blocking the user.

## Purpose
The purpose of Modal is to focus user attention on a contained task, decision, or message.

## Visual Structure
Modal consists of a backdrop, a centered or anchored dialog surface, a clear heading, body content, and action controls. It may include a close affordance and supporting text, but the content should remain concise enough to preserve focus and avoid feeling like a full page hidden inside an overlay.

## Subcomponents
- Layout-Containers
- Heading
- Text
- Paragraph
- Button
- Optional Icon
- Optional close control

## States & Variants
- States: closed, opening, open, focus-trapped, submitting, error, closing.
- Variants: confirmation, informational, form modal, destructive confirmation.
- Sizes: small, medium, large.

## Properties
- `title`: primary dialog heading.
- `body`: supporting content or embedded task.
- `actions`: primary and secondary controls.
- `dismissible`: whether the user can close the modal without the main action.
- `size`: dialog width and density.
- `backdrop`: visual strength of the overlay.
- `initialFocus`: first focus target.
- `status`: optional inline submission or error feedback.

## Brand Reference
No formal brand specification is required for this draft. Modal should remain calm, highly legible, and clearly structured despite being interruptive. Hierarchy, spacing, and clear actions should do most of the work, with visual emphasis used sparingly and only when it improves comprehension.
