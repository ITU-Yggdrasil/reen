# Table

## Description
Table is a structured data-display component for presenting comparable information across rows and columns. Use it when users need to scan, compare, sort, or reference structured data, and avoid using it when the content is primarily narrative or too narrow to justify a grid.

## Purpose
The purpose of Table is to present structured data in a format that supports scanning and comparison.

## Visual Structure
Table includes a header row, repeated body rows, and clearly aligned columns with consistent spacing. It may contain inline actions, status markers, or links, but the grid should preserve legibility and predictable alignment before adding visual complexity.

## Subcomponents
- Text
- Link
- Button
- Optional Icon
- Optional status indicator content

## States & Variants
- States: default, hover row, selected row, empty, loading, sortable, error.
- Variants: simple data table, action table, compact table, comparison table.
- Density options: comfortable or compact.

## Properties
- `columns`: column definitions and labels.
- `rows`: displayed data records.
- `caption`: optional summary of the table's purpose.
- `rowActions`: optional actions per row.
- `sortable`: whether columns support sorting.
- `emptyState`: fallback content when there is no data.
- `density`: spacing mode.
- `selection`: whether rows can be selected.

## Brand Reference
No formal brand specification is required for this draft. Table should make structured information easy to scan and compare through alignment, spacing, and readable typography. Visual separators and emphasis should stay subtle, with clarity taking priority over custom visual expression.
