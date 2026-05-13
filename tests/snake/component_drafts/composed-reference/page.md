# Page

## Description
Page is the top-level composition component for building a complete site view out of the existing design-system components. Use it to define the full page shell and the relationship between navigation, content, supporting regions, and overlays, and avoid turning it into a one-off mockup with bespoke child behavior.

## Purpose
The purpose of Page is to organize all other site components into a coherent full-page structure.

## Visual Structure
Page is a full-page shell with clear regions such as navigation, page header, main content, supporting content, and footer. The structure should feel stable and readable at a glance, with one dominant heading and enough flexibility to hold cards, forms, tables, media, actions, and overlays without losing hierarchy.

## Subcomponents
- Layout-Containers
- Navbar
- Heading
- Text
- Paragraph
- Button
- Link
- Badge
- Image
- Card
- Form
- Input
- Label
- Divider
- Table
- Modal
- Icon

## States & Variants
- States: default, loading, empty, error.
- Variants: landing, dashboard, content, workflow.
- Layout options: navbar on or off, supporting content on or off, footer on or off, overlay-capable.

## Properties
- `title`: primary page heading.
- `variant`: dominant page type.
- `showNavbar`: whether top-level navigation is present.
- `showFooter`: whether footer content is present.
- `hasSupportingContent`: whether a supporting region is present.
- `hasOverlay`: whether modal content may appear above the page.
- `status`: optional short context label near the page heading.
- `loading`: whether the page is in a loading state.
- `empty`: whether the primary content region has no content.
- `error`: whether the page is showing an error state.

## Brand Reference
No formal brand specification is required for this draft. Page should use clear hierarchy, predictable section rhythm, and stable composition so the full site feels easy to scan and understand. Structure and spacing should do most of the work, with the child components carrying local detail and interaction.
