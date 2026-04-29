# Image - Component Specification

## Component Metadata

### Name

Image

### Description

An Image is a media component used to present a preview, illustration, or supporting visual. It should be used when a card or other composed component needs a recognizable visual anchor without adding interaction on its own.

---

## Visual Structure

The Image is a rectangular media surface with predictable aspect ratio handling.

### Layout Structure

Block-level media with either cropped or contained presentation depending on context.

### Content Areas or Slots

- **Media slot (required):** The image content itself.

### Alignment and Spacing Rules

- The image fills its allotted slot cleanly.
- Cropping or containment should preserve the intended focal point.

---

## Variants

- **Thumbnail:** Used for compact card previews.
- **Hero:** Used for larger feature surfaces.

---

## States

### Default

The image renders normally.

### Loading

The image area may show a placeholder while content loads.

### Disabled

The image may appear muted when it is not available.

---

## Properties

- `src`: String. Required. The media source.
- `alt`: String. Required. The accessible description.
- `variant`: `thumbnail` | `hero`.

---

## Accessibility Notes

### ARIA Roles and Accessibility Considerations

- The image must always have meaningful alternative text when it conveys information.
