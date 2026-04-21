# Context: AdsbReceiverContext

## Purpose

AdsbReceiver is the boundary adapter for the ADS-B Exchange REST API. It polls the API at a regular interval, translates each returned AircraftState into a PositionEvent, and forwards those events into the wider system. This context fulfills the EventSource role — downstream contexts cannot distinguish which receiver is active.

## Role Players

- event_sink: EventBufferContext — receives each successfully mapped aircraft event as a PositionEvent

## Props

- api_url: base URL of the ADS-B Exchange aircraft endpoint
- api_key: API key used to authenticate with ADS-B Exchange, sent as a request header
- poll_interval: how frequently the API is queried
- bounding_box: optional geographic restriction for the query (min_latitude, max_latitude, min_longitude, max_longitude)

## Role Methods

### event_sink
- receive_event: accepts a single PositionEvent and stores it for later querying

## Functionalities

### new
Stores the event sink, API URL, API key, poll interval, and optional bounding box. Does not start polling during construction.

### start
Begins a continuous background loop. Waits for the poll interval to elapse before each fetch cycle. Retains the same configuration and event sink across iterations.

### fetch_aircraft
Issues a GET request to the configured API URL. Includes the API key as a request header. Applies the bounding box when configured. On a successful response, iterates the returned aircraft records and processes each one. On failure, records a diagnostic error count and waits until the next interval.

### on_aircraft_received
Attempts to parse the raw record as an AircraftState. If latitude or longitude is absent, discards the record silently. Maps latitude and longitude directly from the aircraft state when present. Uses the state's timestamp as occurred_at. Sets source to "adsb". Uses the callsign as label when present and non-blank. Falls back to the ICAO hex code as label when the callsign is absent or blank. Delivers the resulting PositionEvent to the event sink.

## Scenarios

### blank callsign uses ICAO fallback
Given: an aircraft record with blank callsign and valid coordinates
When: on_aircraft_received runs
Then: a PositionEvent is emitted with the ICAO hex code as its label

### missing coordinates are silently dropped
Given: an aircraft record with no latitude
When: on_aircraft_received runs
Then: no PositionEvent is emitted

### successful poll delivers events
Given: a successful response containing four aircraft with valid coordinates
When: fetch_aircraft runs
Then: four PositionEvents with source "adsb" are delivered
