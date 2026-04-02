# EarthquakeEvent

## Description

EarthquakeEvent is the raw record as received from the USGS Earthquake Hazards GeoJSON
feed before any mapping has taken place. It belongs only at the boundary of the
EarthquakeReceiverContext and must not flow further into the system.

The EarthquakeReceiverContext is solely responsible for reading this record and producing
a PositionEvent from it. Once that mapping is done, the EarthquakeEvent is discarded.

Each record represents one seismic event as detected and catalogued by the USGS network.
The feed is updated approximately once per minute and covers events worldwide.

## Fields

| Field | Meaning | Accessible | Notes |
|---|---|---|---|
| latitude | Epicentre latitude in decimal degrees | X | Extracted from the second GeoJSON coordinate value |
| longitude | Epicentre longitude in decimal degrees | X | Extracted from the first GeoJSON coordinate value |
| depth | Depth in kilometres below the surface |  | Informational only |
| timestamp | UTC origin time of the earthquake | X | Receiver converts from milliseconds since epoch |
| magnitude | Optional preferred magnitude | X | Used when forming the event label |
| place | Optional human-readable location description | X | Used when forming the event label |
