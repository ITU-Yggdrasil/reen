# Data: GridResolution

## Purpose

GridResolution controls how large each GeographicCell is and therefore how many cells the world is divided into. It is expressed as two angular sizes in degrees: one for the latitude axis and one for the longitude axis. The default is ten degrees on both axes, which produces an 18 by 36 grid of 648 cells covering the entire globe. It is a configuration value, set once at startup and held constant for the lifetime of the process.

## Attributes

- degrees_latitude: height of each cell in decimal degrees of latitude; default is ten degrees
- degrees_longitude: width of each cell in decimal degrees of longitude; default is ten degrees

## Invariants

- Both values must be positive.
- 180 divided by degrees_latitude must be a whole number (the latitude extent divides evenly).
- 360 divided by degrees_longitude must be a whole number (the longitude extent divides evenly).

## Operations

### new
Constructs a GridResolution from the provided axis sizes.

## Scenarios

### default resolution
Given: degrees_latitude is 10 and degrees_longitude is 10
When: new is called
Then: a GridResolution is returned that produces 648 cells globally

### uneven division is rejected
Given: degrees_latitude is 7
When: new is called
Then: construction fails because 180 divided by 7 is not a whole number
