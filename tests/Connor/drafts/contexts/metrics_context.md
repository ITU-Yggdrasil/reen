# MetricsContext

## Purpose

MetricsContext owns the HTTP server and is responsible for serving two kinds of responses:

1. A Prometheus scrape endpoint that renders the current aggregation data as gauge metrics.
2. A JSON query endpoint that returns the current live events in a form suitable for
   the map-based test visualisation.

On each Prometheus scrape the context asks the aggregator for current rates, then encodes
them in the Prometheus text exposition format with geographic labels. The JSON endpoint
reads the current buffered events and serialises them as dashboard event objects.

MetricsContext has no knowledge of events, geography, or feeds — it only knows how to
ask its collaborators for a result and serialise it into the appropriate wire format.

## Role Players

| Role player | Why involved | Expected behaviour |
|---|---|---|
| aggregator | Supplies on-demand aggregation results | Returns EventRate values for every cell |
| buffer | Supplies current live PositionEvents | Returns current events, optionally filtered by source |

## Role Methods

### aggregator

- **produce_rates** Returns one EventRate per cell, covering all cells in the grid.

### buffer

- **current_events_for_source(source)** Returns the current live PositionEvents, optionally filtered by source.

## Props

## Functionalities

### new

| Started by | Uses | Result |
|---|---|---|
| application startup | aggregator, buffer | metrics context is constructed |

Rules:
- Stores the provided aggregator and buffer.
- Does not bind a socket or start an HTTP server at construction time.

| Given | When | Then |
|---|---|---|
| an aggregator and buffer are available | new is called | a MetricsContext is returned |

### handle_metrics_request

| Started by | Uses | Result |
|---|---|---|
| HTTP GET to metrics_path | aggregator | Prometheus response is returned |

Rules:
- Calls `aggregator.produce_rates`.
- Encodes each EventRate as a Prometheus gauge metric named `geospatial_event_rate`.
- Each metric line carries labels `min_lat`, `max_lat`, `min_lon`, and `max_lon`.
- Uses `events_per_minute` as the gauge value.
- Includes zero-valued cells in the output.
- Returns the full metric text with a 200 status and the Prometheus content type.

| Given | When | Then |
|---|---|---|
| the aggregator returns a non-zero rate for one cell | handle_metrics_request runs | the response body contains a `geospatial_event_rate` line with the correct labels and value |

### handle_query_request

| Started by | Uses | Result |
|---|---|---|
| HTTP GET to query_path | buffer, optional source filter | JSON event array is returned |

Rules:
- Calls `buffer.current_events_for_source(source)`.
- Treats absent source as all sources.
- Accepts `flight`, `lightning`, `earthquake`, `aisstream`, `adsb`, and `wiki` as explicit filters.
- Treats any other source query value the same as absent.
- Returns a JSON array of objects.
- Each object contains `latitude`, `longitude`, `occurred_at`, `source`, and `label`.
- Encodes `occurred_at` as RFC 3339 text.
- Encodes `label` as `null` when absent.
- Returns the JSON payload with a 200 status and an appropriate content type.

| Given | When | Then |
|---|---|---|
| a GET request arrives at query_path with `source=lightning` | handle_query_request runs | only lightning events are included in the returned JSON array |
