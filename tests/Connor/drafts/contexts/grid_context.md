# GridContext

## Purpose

GridContext owns the geographic grid that the world is divided into.
It knows how to map any latitude/longitude coordinate to the correct GeographicCell,
and it can enumerate all cells in the grid.

The grid is constructed once at startup from the configured GridResolution and does
not change during the lifetime of the process. GridContext has no knowledge of events,
counts, or rates — its sole responsibility is the spatial organisation of the world
into cells.

All other contexts that need to think geographically — primarily AggregationContext —
ask GridContext for the appropriate cell rather than computing cell membership
themselves.

## Role Players

## Role Methods

## Props

| Prop | Meaning | Notes |
|---|---|---|
| resolution | GridResolution that determines the size of each cell | Defaults to ten degrees on both axes |

## Functionalities

### new

| Started by | Uses | Result |
|---|---|---|
| application startup | resolution | grid is constructed or construction fails |

Rules:
- Uses a fixed grid origin of `-90.0` latitude and `-180.0` longitude.
- Lays out cell bands by stepping northward and eastward from those origins.
- Construction is valid only when `180.0 / resolution.degrees_latitude` is an integer.
- Construction is valid only when `360.0 / resolution.degrees_longitude` is an integer.
- If either extent is not divided exactly, construction fails instead of producing a ragged grid.
- The grid is fixed after construction.

| Given | When | Then |
|---|---|---|
| a resolution whose latitude or longitude does not evenly divide the world extent | new is called | construction fails |

### cell_for

| Started by | Uses | Result |
|---|---|---|
| caller with coordinates | resolution, grid lattice | containing GeographicCell is returned |

Rules:
- Returns the GeographicCell whose bounds contain the given latitude and longitude.
- Valid latitude range is `[-90.0, 90.0]`.
- Valid longitude range is `[-180.0, 180.0]`.
- Inputs outside those ranges are clamped to the nearest valid bound before assignment.
- Every coordinate after clamping maps to exactly one cell.
- Interior boundary values belong to the cell for which that boundary is the lower bound.
- Latitude `90.0` maps to the northernmost band.
- Longitude `180.0` maps to the easternmost band.

| Given | When | Then |
|---|---|---|
| the default resolution and coordinate 51.5°N, 0.1°W | cell_for is called | the returned cell has min_latitude 50°, max_latitude 60°, min_longitude −10°, max_longitude 0° |

### all_cells

| Started by | Uses | Result |
|---|---|---|
| caller needing the full grid | resolution, grid lattice | complete ordered cell list is returned |

Rules:
- Returns the complete list of GeographicCells in the grid.
- Total cell count is determined by how often the resolution divides 180 degrees of latitude and 360 degrees of longitude.
- Cells are enumerated from south to north and, within each latitude band, from west to east.
- For the default ten-degree resolution, the total is 648 cells.

| Given | When | Then |
|---|---|---|
| the default ten-degree resolution | all_cells is called | exactly 648 cells are returned |

## Notes

Cell boundaries are aligned to the fixed global lattice starting at `-90.0` latitude and
`-180.0` longitude. There is no wrap-around behaviour at `180.0`; that coordinate belongs
to the final easternmost cell.
