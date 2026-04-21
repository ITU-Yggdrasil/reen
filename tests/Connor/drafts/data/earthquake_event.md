# Data: EarthquakeEvent

## Purpose

EarthquakeEvent is the raw record received from the USGS Earthquake Hazards GeoJSON feed before any mapping has taken place. It belongs only at the boundary of the EarthquakeReceiver context and must not flow further into the system. Each record represents one seismic event as detected and catalogued by the USGS network.

## Attributes

- latitude: epicentre latitude in decimal degrees, extracted from the GeoJSON coordinates (note: GeoJSON coordinate order is longitude, latitude, depth)
- longitude: epicentre longitude in decimal degrees, extracted from the GeoJSON coordinates
- depth: depth in kilometres below the surface, informational only
- timestamp: UTC origin time of the earthquake, converted from milliseconds since epoch by the receiver
- magnitude: optional preferred magnitude, used when forming the event label
- place: optional human-readable location description, used when forming the event label

## Invariants

## Operations

## Scenarios

### earthquake with magnitude and place
Given: latitude 35.6, longitude -117.5, depth 10.0, a UTC timestamp, magnitude 4.2, and place "20km NW of Ridgecrest, CA"
When: the receiver maps this record
Then: the label is formed from magnitude and place
