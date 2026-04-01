# EarthquakeEvent

## Description

EarthquakeEvent is the raw record as received from the USGS Earthquake Hazards GeoJSON
feed before any mapping has taken place. It belongs only at the boundary of the
EarthquakeReceiverContext and must not flow further into the system.

The EarthquakeReceiverContext is solely responsible for reading this record and producing
a PositionEvent from it. Once that mapping is done, the EarthquakeEvent is discarded.

Each record represents one seismic event as detected and catalogued by the USGS network.
The feed is updated approximately once per minute and covers events worldwide.

---

## Fields

latitude, longitude, and timestamp are readable by EarthquakeReceiverContext when mapping
an event to a PositionEvent. magnitude and place are used to form the label. depth is not
read during mapping.

- **latitude**
  The geographic latitude of the earthquake's epicentre in decimal degrees.
  Extracted from the second element of the GeoJSON coordinates array.
  Read by EarthquakeReceiverContext when setting the latitude of the resulting PositionEvent.

- **longitude**
  The geographic longitude of the earthquake's epicentre in decimal degrees.
  Extracted from the first element of the GeoJSON coordinates array (GeoJSON uses
  [longitude, latitude, depth] order, which is the reverse of the conventional order).
  Read by EarthquakeReceiverContext when setting the longitude of the resulting PositionEvent.

- **depth**
  The depth of the earthquake in kilometres below the surface.
  Extracted from the third element of the GeoJSON coordinates array.
  Carried for informational completeness; not used in aggregation logic.

- **timestamp**
  The origin time of the earthquake as reported by USGS, stored as a UTC timestamp. The
  receiver is responsible for converting from the USGS wire format (milliseconds since
  Unix epoch) into a UTC timestamp before constructing the struct.
  Read by EarthquakeReceiverContext when setting the occurred_at of the resulting PositionEvent.

- **magnitude**
  The preferred magnitude of the earthquake (e.g., Richter, moment magnitude). May be
  absent for very recent events still being processed. When present, included in the label.
  Read by EarthquakeReceiverContext when forming the label of the resulting PositionEvent.

- **place**
  A human-readable description of the earthquake's location relative to nearby populated
  places, as provided by USGS (e.g., "10km NNE of Ridgecrest, CA"). May be absent.
  When present, included in the label alongside the magnitude.
  Read by EarthquakeReceiverContext when forming the label of the resulting PositionEvent.
