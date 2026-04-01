# PositionEvent

## Description

PositionEvent is the feed-agnostic primitive that the rest of the system works with.
It represents a single detected occurrence at a point on Earth at a moment in time.

Lightning strikes, flight positions, earthquake detections, AISStream vessel positions, ADS-B
aircraft states, and Wikipedia edit events may all be mapped into this form at the
boundary of their respective receiver contexts. Once in this form, the event is processed
uniformly by the buffer, grid, and aggregator layers, but it still retains a source field
identifying which upstream feed produced it.

---

## Fields

All fields are private. Collaborators read them only through the getter methods listed in
`Functionalities`.

- **latitude**
  The north–south coordinate of the event in decimal degrees.
  Positive values are north of the equator; negative values are south.
  Read by AggregationContext when mapping the event to a GeographicCell.

- **longitude**
  The east–west coordinate of the event in decimal degrees.
  Positive values are east of the prime meridian; negative values are west.
  Read by AggregationContext when mapping the event to a GeographicCell.

- **occurred_at**
  The moment the event took place, expressed as a UTC timestamp.
  For lightning, this is the strike timestamp supplied by the network.
  For flights, this is the observation timestamp supplied by the API.
  For earthquakes, AISStream vessel positions, ADS-B aircraft, and Wikipedia edits, this is the timestamp
  supplied by the upstream feed or derived boundary record.
  Must be UTC so that comparisons with the current time are unambiguous.
  Read by EventBufferContext when comparing against the staleness boundary.

- **source**
  The canonical event source identifier.
  Allowed values are:
  - `lightning`
  - `flight`
  - `earthquake`
  - `aisstream`
  - `adsb`
  - `wiki`
  This field is set by the receiver that constructs the PositionEvent.
  It is read by EventBufferContext when filtering by source and by MetricsContext when
  serialising live events to JSON.

- **label**
  An optional human-readable tag for diagnostics or display.
  Used only for diagnostic and labelling purposes;
  the system's aggregation logic must not vary based on this value.

---

## Construction rules

- Every PositionEvent must include a source.
- The source is part of the event's data model, not an inferred property.
- Filtering by source must use the source field, not the label.

---

## Functionalities

- **new(latitude, longitude, occurred_at, source, label)**
  Constructs a PositionEvent from the supplied field values.

- **latitude()**
  Returns the event latitude.

- **longitude()**
  Returns the event longitude.

- **occurred_at()**
  Returns the event timestamp.

- **source()**
  Returns the canonical event source identifier.

- **label()**
  Returns the optional human-readable label.
