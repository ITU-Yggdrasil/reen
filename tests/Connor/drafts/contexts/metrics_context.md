# MetricsContext

## Description

MetricsContext owns the HTTP server and is responsible for serving two kinds of responses:

1. A Prometheus scrape endpoint that renders the current aggregation data as gauge metrics.
2. A JSON query endpoint that returns the current live events in a form suitable for
   the map-based test visualisation.

On each Prometheus scrape the context asks the aggregator for current rates, then encodes
them in the Prometheus text exposition format with geographic labels. The JSON endpoint
reads the current buffered events and serialises them as dashboard event objects.

MetricsContext has no knowledge of events, geography, or feeds — it only knows how to
ask its collaborators for a result and serialise it into the appropriate wire format.

---

## Roles

- **aggregator**
  Provides on-demand aggregation results.
  Fulfilled by AggregationContext.

- **buffer**
  Provides the current live PositionEvents, optionally filtered by source.
  Fulfilled by EventBufferContext.

---

## Role methods

### aggregator

- **produce_rates**
  Returns one EventRate per cell, covering all cells in the grid.

### buffer

- **current_events_for_source(source)**
  Returns the current live PositionEvents, optionally filtered by source.

---

## Functionalities

- **new(aggregator, buffer)**
  Constructs a MetricsContext with the given role players.
  Stores the aggregator and buffer. Does not itself bind a socket or start an HTTP server.

- **handle_metrics_request**
  Called when an HTTP GET arrives at the configured metrics_path.
  Calls aggregator.produce_rates.
  Encodes each EventRate as a Prometheus gauge metric named `geospatial_event_rate`.
  Each metric line carries the following labels:
  - `min_lat` — the cell's southern latitude boundary,
  - `max_lat` — the cell's northern latitude boundary,
  - `min_lon` — the cell's western longitude boundary,
  - `max_lon` — the cell's eastern longitude boundary.
  The gauge value is the events_per_minute from the EventRate.
  Cells with a rate of zero are included in the output so that Prometheus can observe
  the disappearance of activity.
  Returns the full metric text with a 200 status and the Prometheus content-type header.

- **handle_query_request**
  Called when an HTTP GET arrives at the configured query_path.
  Calls buffer.current_events_for_source(source), where source is either:
  - absent, meaning all sources,
  - `flight`,
  - `lightning`,
  - `earthquake`,
  - `aisstream`,
  - `adsb`,
  - `wiki`.
  Any other source query value is treated the same as absent: no source filter is applied
  and events from all sources are returned.
  Returns a JSON array.
  Each array element is a JSON object, not a positional array.
  The exact object fields are:
  - `latitude`
  - `longitude`
  - `occurred_at`
  - `source`
  - `label`
  Field meanings:
  - `latitude`: numeric latitude in decimal degrees.
  - `longitude`: numeric longitude in decimal degrees.
  - `occurred_at`: timestamp string in RFC 3339 format.
  - `source`: string event source name.
  - `label`: string label when present, or `null` when absent.
  Returns the JSON payload with a 200 status and an appropriate content-type header.

---

## Acceptance examples

- Given the aggregator returns a non-zero rate for one cell, when a Prometheus scrape
  arrives, then the response body contains exactly one non-zero `geospatial_event_rate`
  line carrying the correct cell labels and value.
- Given the aggregator returns rates for 648 cells, when a Prometheus scrape arrives,
  then the response contains 648 metric lines (including any zero-valued cells).
- Given a GET request arrives at query_path, when handle_query_request runs, then the
  response is valid JSON and each entry is an object with fields `latitude`,
  `longitude`, `occurred_at`, `source`, and `label`.
- Given a GET request arrives at query_path with `source=lightning`, when
  handle_query_request runs, then only lightning events are included in the returned
  JSON array.
- Given a GET request arrives at query_path with `source=wiki`, when
  handle_query_request runs, then only wiki events are included in the returned JSON
  array.
- Given a GET request arrives at query_path with `source=unknown`, when
  handle_query_request runs, then the source value is treated as absent and the returned
  JSON array includes events from all sources.
