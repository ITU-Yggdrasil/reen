# Context: FlightReceiverContext

## Purpose

FlightReceiver is the boundary adapter for the OpenSky Network REST API. It polls the API at a regular interval, translates each returned FlightPosition into a PositionEvent, and forwards those events into the wider system. This context fulfills the EventSource role — downstream contexts cannot distinguish which receiver is active.

## Role Players

- event_sink: EventBufferContext — receives each successfully mapped flight event as a PositionEvent

## Props

- api_url: base URL of the OpenSky Network states endpoint
- api_token: optional bearer token for authenticated OpenSky access; when absent, requests are sent without an authorization header
- poll_interval: how frequently the API is queried
- bounding_box: optional geographic restriction for the query (min_latitude, max_latitude, min_longitude, max_longitude)

## Role Methods

### event_sink
- receive_event: accepts a single PositionEvent and stores it for later querying

## Functionalities

### new
Stores the event sink, API URL, optional API token, poll interval, and optional bounding box. Does not start polling during construction.

### start
Begins a continuous background loop. Waits for the poll interval to elapse before each fetch cycle. Retains the same configuration and event sink across iterations.

### fetch_positions
Issues a GET request to the configured API URL. Applies the bounding box when configured. Includes the bearer token as an authorization header when present. On a successful response, iterates the returned flight state records and processes each one. On failure, records a diagnostic error count and waits until the next interval.

### on_position_received
Attempts to parse the raw record as a FlightPosition. If latitude or longitude is absent, discards the record silently. Maps latitude and longitude directly from the flight position when present. Uses the flight position timestamp as occurred_at. Sets source to "flight". Uses the callsign as label when present; falls back to the literal label "flight" when absent. Delivers the resulting PositionEvent to the event sink.

## Scenarios

### successful poll with valid positions
Given: a successful API response containing three valid positions
When: fetch_positions runs
Then: three PositionEvents with source "flight" are delivered

### missing coordinates are silently dropped
Given: a position record with no latitude
When: on_position_received runs
Then: no PositionEvent is emitted

### absent callsign uses fallback label
Given: a flight record without a callsign
When: on_position_received runs
Then: the PositionEvent label is "flight"
