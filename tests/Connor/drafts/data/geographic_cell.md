# GeographicCell

## Description

GeographicCell is the unit of geographic aggregation.
It represents a rectangular region of the Earth's surface defined by a pair of latitude bounds
and a pair of longitude bounds.

The grid divides the entire world into a fixed number of non-overlapping cells whose size is
determined by the configured GridResolution. Every point on Earth belongs to exactly one cell.
Cells are identified by their south-west corner, which serves as their canonical coordinate
within the grid.

## Fields

| Field | Meaning | Accessible | Notes |
|---|---|---|---|
| min_latitude | Southern edge of the cell | X | Used in metrics labels and grid calculations |
| max_latitude | Northern edge of the cell | X | Used in metrics labels and grid calculations |
| min_longitude | Western edge of the cell | X | Used in metrics labels and grid calculations |
| max_longitude | Eastern edge of the cell | X | Used in metrics labels and grid calculations |

## Functionalities

- **new(min_latitude, max_latitude, min_longitude, max_longitude)** Constructs a GeographicCell from the four boundary values.
