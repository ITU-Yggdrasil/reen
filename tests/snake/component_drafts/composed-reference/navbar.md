# Navbar

## Description
Navbar is the primary top-level navigation component for exposing major destinations, brand identity, and utility actions. Use it to orient the user and provide reliable global access points, and avoid overloading it with dense secondary content that belongs elsewhere in the layout.

## Purpose
The purpose of Navbar is to provide orientation and access to the most important navigation paths.

## Visual Structure
Navbar typically includes brand identification on one side, a set of navigation links, and optional utility actions such as buttons or icons. The layout should remain horizontally organized, visually stable, and adaptable to reduced space without losing the most important navigation choices.

## Subcomponents
- Layout-Containers
- Link
- Button
- Icon
- Optional Heading or brand mark area

## States & Variants
- States: default, scrolled, active-link, collapsed, expanded, sticky.
- Variants: simple, product, marketing, utility-heavy.
- Responsive patterns: inline navigation, overflow menu, stacked mobile arrangement.

## Properties
- `brand`: brand name or mark content.
- `items`: primary navigation entries.
- `actions`: optional utility controls.
- `activeItem`: currently selected destination.
- `sticky`: whether the navbar remains fixed during scroll.
- `responsiveMode`: adaptation strategy for narrow viewports.
- `alignment`: content distribution pattern.

## Brand Reference
No formal brand specification is required for this draft. Navbar should rely on strong structure, readable typography, and uncluttered composition so orientation is immediate. Current location and important actions should be clear without depending on a custom brand palette or decorative treatment.
