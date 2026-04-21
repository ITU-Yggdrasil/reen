# Context: MetricsContext

## Purpose

Metrics owns the HTTP server and is responsible for serving two kinds of responses: a Prometheus scrape endpoint that renders the current aggregation data as gauge metrics, and a JSON query endpoint that returns the current live events for the map-based visualisation. Metrics has no knowledge of events, geography, or feeds — it only knows how to ask its collaborators for a result and serialise it into the appropriate wire format.

## Role Players

- aggregator: AggregationContext — supplies on-demand aggregation results as EventRate values for every cell
- buffer: EventBufferContext — supplies current live PositionEvents, optionally filtered by source

## Props

## Role Methods

### aggregator
- produce_rates: returns one EventRate per cell, covering all cells in the grid

### buffer
- current_events_for_source: returns the current live PositionEvents, optionally filtered by source

## Functionalities

### new
Stores the provided aggregator and buffer. Does not bind a socket or start an HTTP server at construction time.

### handle_metrics_request
Asks the aggregator for the current rates. Encodes each EventRate as a Prometheus gauge metric named "geospatial_event_rate". Each metric line carries labels min_lat, max_lat, min_lon, and max_lon. Uses events_per_minute as the gauge value. Includes zero-valued cells in the output. Returns the full metric text with a 200 status and the Prometheus content type.

### handle_query_request
Asks the buffer for the current events, optionally filtered by source. Treats absent source as all sources. Accepts "flight", "lightning", "earthquake", "aisstream", "adsb", and "wiki" as explicit filters. Treats any other source value the same as absent. Returns a JSON array of objects, each containing latitude, longitude, occurred_at (as RFC 3339 text), source, and label (as absent when not present). Returns the JSON payload with a 200 status and an appropriate content type.

## Scenarios

### prometheus scrape with non-zero rate
Given: the aggregator returns a non-zero rate for one cell
When: handle_metrics_request is called
Then: the response body contains a geospatial_event_rate line with the correct labels and value

### query filtered by source
Given: a request arrives with source "lightning"
When: handle_query_request is called
Then: only lightning events are included in the returned JSON array

### query without source filter
Given: a request arrives without a source parameter
When: handle_query_request is called
Then: events from all sources are included
