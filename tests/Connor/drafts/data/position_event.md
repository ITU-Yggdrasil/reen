# PositionEvent

## Description

PositionEvent is the feed-agnostic primitive that the rest of the system works with.
It represents a single detected occurrence at a point on Earth at a moment in time.

Lightning strikes, flight positions, earthquake detections, AISStream vessel positions, ADS-B
aircraft states, and Wikipedia edit events may all be mapped into this form at the
boundary of their respective receiver contexts. Once in this form, the event is processed
uniformly by the buffer, grid, and aggregator layers, but it still retains a source field
identifying which upstream feed produced it.

## Fields

| Field | Meaning | Accessible | Notes |
|---|---|---|---|
| latitude | North-south coordinate in decimal degrees | X | Used for grid cell lookup |
| longitude | East-west coordinate in decimal degrees | X | Used for grid cell lookup |
| occurred_at | UTC timestamp for when the event occurred | X | Used for buffer staleness checks |
| source | Canonical event source identifier | X | Allowed values are `lightning`, `flight`, `earthquake`, `aisstream`, `adsb`, `wiki` |
| label | Optional human-readable tag | X | Diagnostic and display only |

## Construction Rules

- Every PositionEvent includes a source.
- The source is part of the event's data model, not an inferred property.
- Filtering by source uses the source field, not the label.

## Functionalities

- **new(latitude, longitude, occurred_at, source, label)** Constructs a PositionEvent from the supplied field values.
