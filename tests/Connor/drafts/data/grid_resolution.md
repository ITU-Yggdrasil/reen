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

## Fields

| Field | Meaning | Accessible | Notes |
|---|---|---|---|
| degrees_latitude | Height of each cell in decimal degrees of latitude | X | Default is ten degrees |
| degrees_longitude | Width of each cell in decimal degrees of longitude | X | Default is ten degrees |

## Functionalities

- **new(degrees_latitude, degrees_longitude)** Constructs a GridResolution from the provided axis sizes.
