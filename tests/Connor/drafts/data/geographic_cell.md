# GeographicCell

## Description

GeographicCell is the unit of geographic aggregation.
It represents a rectangular region of the Earth's surface defined by a pair of latitude bounds
and a pair of longitude bounds.

The grid divides the entire world into a fixed number of non-overlapping cells whose size is
determined by the configured GridResolution. Every point on Earth belongs to exactly one cell.
Cells are identified by their south-west corner, which serves as their canonical coordinate
within the grid.

---

## Fields

All fields are private. Collaborators read them only through the getter methods listed in
`Functionalities`.

- **min_latitude**
  The southern edge of the cell in decimal degrees.
  Read by MetricsContext when generating Prometheus labels and JSON boundary fields,
  and by GridContext when computing cell boundaries during grid construction.

- **max_latitude**
  The northern edge of the cell in decimal degrees.
  Read by the same collaborators as min_latitude.

- **min_longitude**
  The western edge of the cell in decimal degrees.
  Read by the same collaborators as min_latitude.

- **max_longitude**
  The eastern edge of the cell in decimal degrees.
  Read by the same collaborators as min_latitude.

---

## Functionalities

- **new(min_latitude, max_latitude, min_longitude, max_longitude)**
  Constructs a GeographicCell from the four boundary values.

- **min_latitude()**
  Returns the southern edge of the cell.

- **max_latitude()**
  Returns the northern edge of the cell.

- **min_longitude()**
  Returns the western edge of the cell.

- **max_longitude()**
  Returns the eastern edge of the cell.
