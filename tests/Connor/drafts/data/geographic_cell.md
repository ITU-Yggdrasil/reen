# Data: GeographicCell

## Purpose

GeographicCell is the unit of geographic aggregation. It represents a rectangular region of the Earth's surface defined by latitude and longitude bounds. The grid divides the entire world into non-overlapping cells whose size is determined by the configured GridResolution. Every point on Earth belongs to exactly one cell.

## Attributes

- min_latitude: southern edge of the cell in decimal degrees
- max_latitude: northern edge of the cell in decimal degrees
- min_longitude: western edge of the cell in decimal degrees
- max_longitude: eastern edge of the cell in decimal degrees

## Invariants

- min_latitude is less than max_latitude.
- min_longitude is less than max_longitude.
- Cell boundaries align to the fixed global lattice starting at -90 degrees latitude and -180 degrees longitude.

## Operations

### new
Constructs a GeographicCell from the four boundary values.

## Scenarios

### cell covering London
Given: min_latitude 50, max_latitude 60, min_longitude -10, max_longitude 0
When: new is called
Then: a GeographicCell covering that region is returned
