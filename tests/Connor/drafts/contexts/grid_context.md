# GridContext

## Description

GridContext owns the geographic grid that the world is divided into.
It knows how to map any latitude/longitude coordinate to the correct GeographicCell,
and it can enumerate all cells in the grid.

The grid is constructed once at startup from the configured GridResolution and does
not change during the lifetime of the process. GridContext has no knowledge of events,
counts, or rates — its sole responsibility is the spatial organisation of the world
into cells.

All other contexts that need to think geographically — primarily AggregationContext —
must ask GridContext for the appropriate cell rather than computing cell membership
themselves.

---

## Props

- **resolution**
  The GridResolution that determines the size of each cell.
  Defaults to ten degrees of latitude and ten degrees of longitude.

---

## Functionalities

- **new(resolution)**
  Constructs the grid from the provided GridResolution.
  The grid origin is fixed at:
  - latitude origin: `-90.0`
  - longitude origin: `-180.0`
  Cell bands are laid out by stepping northward and eastward from those origins in
  increments of the configured resolution.
  Construction is valid only when:
  - `180.0 / resolution.degrees_latitude` is an integer, and
  - `360.0 / resolution.degrees_longitude` is an integer.
  If either extent is not divided exactly, construction fails rather than producing a
  partial or ragged grid.
  The grid is fixed at construction time and does not change during the lifetime of
  the process.

- **cell_for(latitude, longitude)**
  Returns the GeographicCell that contains the given coordinate.
  The cell is the one whose latitude bounds contain the given latitude and whose
  longitude bounds contain the given longitude.
  Valid input ranges are:
  - latitude: `[-90.0, 90.0]`
  - longitude: `[-180.0, 180.0]`
  Inputs outside those ranges are not wrapped or normalised cyclically; they are
  clamped to the nearest valid bound before cell assignment.
  Every coordinate after clamping maps to exactly one cell.
  Coordinates on a cell boundary (exactly at a min or max edge) are assigned to the
  cell for which that value is the lower bound — i.e. interior boundary values belong
  to the cell to the north and east.
  The global upper bounds are special cases:
  - latitude `90.0` maps to the northernmost latitude band,
  - longitude `180.0` maps to the easternmost longitude band.
  The global lower bounds map naturally to the first bands:
  - latitude `-90.0` maps to the southernmost latitude band,
  - longitude `-180.0` maps to the westernmost longitude band.

- **all_cells**
  Returns the complete list of GeographicCells that make up the grid.
  The total number of cells is determined by how many times the resolution divides
  into 180 degrees of latitude and 360 degrees of longitude.
  Cells are enumerated from south to north, and within each latitude band from west
  to east.
  For the default ten-degree resolution this is 648 cells (18 latitude bands × 36
  longitude bands).

---

## Grid rules

- Cell boundaries are aligned to the fixed global lattice starting at `-90.0` latitude
  and `-180.0` longitude.
- For latitude band index `i`, bounds are:
  - `min_latitude = -90.0 + i * degrees_latitude`
  - `max_latitude = min_latitude + degrees_latitude`
- For longitude band index `j`, bounds are:
  - `min_longitude = -180.0 + j * degrees_longitude`
  - `max_longitude = min_longitude + degrees_longitude`
- There is no wrap-around behaviour at `180.0`; that coordinate belongs to the final
  easternmost cell.

---

## Acceptance examples

- Given the default resolution and a coordinate of 51.5°N, 0.1°W, when cell_for is
  called, then the returned cell has min_latitude 50°, max_latitude 60°,
  min_longitude −10°, max_longitude 0°.
- Given the default resolution, when all_cells is called, then exactly 648 cells are
  returned.
- Given a resolution of five degrees, when all_cells is called, then exactly 2592 cells
  are returned (36 × 72).
- Given a resolution whose latitude does not evenly divide 180° or whose longitude
  does not evenly divide 360°, when new is called, then construction fails.
- Given a coordinate above 90°N or below 90°S, when cell_for is called, then latitude
  is clamped to the nearest valid pole before determining the cell.
- Given a coordinate of exactly 90°N, when cell_for is called, then the returned cell
  is in the northernmost latitude band.
- Given a coordinate of exactly 180°E, when cell_for is called, then the returned cell
  is in the easternmost longitude band.
