# GridResolution

## Description

GridResolution controls how large each GeographicCell is, and therefore how many cells
the world is divided into.

It is expressed as two angular sizes in degrees: one for the latitude axis and one for
the longitude axis. The default is ten degrees on both axes, which produces an 18×36
grid covering the entire globe. Finer resolutions produce smaller cells and more granular
aggregation; coarser resolutions produce fewer, larger cells.

GridResolution is a configuration value, set once at startup and held constant for the
lifetime of the process.

---

## Fields

Both fields are private. Collaborators read them only through the getter methods listed
in `Functionalities`.

- **degrees_latitude**
  The height of each cell in decimal degrees of latitude.
  The default value is ten degrees.
  Read by GridContext when computing cell boundaries and enumerating the grid.

- **degrees_longitude**
  The width of each cell in decimal degrees of longitude.
  The default value is ten degrees.
  Read by GridContext when computing cell boundaries and enumerating the grid.

---

## Functionalities

- **new(degrees_latitude, degrees_longitude)**
  Constructs a GridResolution from the provided axis sizes.

- **degrees_latitude()**
  Returns the configured latitude step in decimal degrees.

- **degrees_longitude()**
  Returns the configured longitude step in decimal degrees.
