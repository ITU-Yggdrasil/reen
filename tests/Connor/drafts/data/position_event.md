# Data: PositionEvent

## Purpose

PositionEvent is the feed-agnostic primitive that the rest of the system works with. It represents a single detected occurrence at a point on Earth at a moment in time. Lightning strikes, flight positions, earthquake detections, vessel positions, aircraft states, and Wikipedia edit events are all mapped into this form at the boundary of their respective receiver contexts. Once in this form, the event is processed uniformly by the buffer, grid, and aggregator layers.

## Attributes

- latitude: north-south coordinate in decimal degrees, used for grid cell lookup
- longitude: east-west coordinate in decimal degrees, used for grid cell lookup
- occurred_at: UTC timestamp for when the event occurred, used for buffer staleness checks
- source: canonical event source identifier; allowed values are "lightning", "flight", "earthquake", "aisstream", "adsb", "wiki"
- label: optional human-readable tag for diagnostic and display purposes

## Invariants

- Every PositionEvent includes a source.
- The source is part of the event's data model, not an inferred property.
- Filtering by source uses the source field, not the label.

## Operations

### new
Constructs a PositionEvent from latitude, longitude, occurred_at, source, and label.

## Scenarios

### create a lightning event
Given: latitude 51.5, longitude -0.1, a UTC timestamp, source "lightning", and label "lightning"
When: new is called
Then: a PositionEvent is returned with those values

### source identifies the upstream feed
Given: two events, one with source "flight" and one with source "earthquake"
When: events are filtered by source "flight"
Then: only the flight event is included
