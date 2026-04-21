# Data: GridContext

## Purpose

Grid owns the geographic grid that the world is divided into. It knows how to map any latitude and longitude coordinate to the correct GeographicCell and can enumerate all cells. The grid is constructed once at startup from the configured GridResolution and does not change during the lifetime of the process. Grid has no knowledge of events, counts, or rates — its sole responsibility is the spatial organisation of the world into cells.

## Attributes

- resolution: the GridResolution that determines the size of each cell
- cells: the complete pre-computed list of GeographicCells in the grid

## Invariants

- The grid uses a fixed origin of -90 degrees latitude and -180 degrees longitude.
- Cell bands step northward and eastward from the origin.
- 180 divided by the resolution's degrees_latitude must be a whole number.
- 360 divided by the resolution's degrees_longitude must be a whole number.
- The grid is immutable after construction.

## Operations

### new
Constructs a Grid from the provided GridResolution. Lays out cell bands by stepping northward and eastward from the fixed origin. Fails if either extent is not divided evenly by the resolution.

### cell_for
Returns the GeographicCell whose bounds contain the given latitude and longitude. Valid latitude range is -90 to 90 degrees; valid longitude range is -180 to 180 degrees. Inputs outside those ranges are clamped to the nearest valid bound before assignment. Every coordinate after clamping maps to exactly one cell. Interior boundary values belong to the cell for which that boundary is the lower bound. Latitude 90 maps to the northernmost band. Longitude 180 maps to the easternmost band.

### all_cells
Returns the complete list of GeographicCells in the grid. Cells are enumerated from south to north and, within each latitude band, from west to east. For the default ten-degree resolution, the total is 648 cells.

## Scenarios

### default resolution produces 648 cells
Given: a GridResolution of ten degrees on both axes
When: new is called
Then: a Grid with 648 cells is returned

### uneven resolution is rejected
Given: a GridResolution where degrees_latitude is 7
When: new is called
Then: construction fails because 180 divided by 7 is not a whole number

### London maps to correct cell
Given: the default ten-degree resolution
When: cell_for is called with latitude 51.5 and longitude -0.1
Then: the returned cell has min_latitude 50, max_latitude 60, min_longitude -10, max_longitude 0

### clamping at the north pole
Given: latitude 91 (out of range)
When: cell_for is called
Then: the latitude is clamped to 90 and maps to the northernmost band
