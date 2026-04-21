# Context: EarthquakeReceiverContext

## Purpose

EarthquakeReceiver is the boundary adapter for the USGS Earthquake Hazards GeoJSON feed. It polls the API at a regular interval, translates each returned EarthquakeEvent into a PositionEvent, and forwards those events into the wider system. Because the USGS feed returns overlapping windows of events, the receiver deduplicates by event ID before forwarding.

## Role Players

- event_sink: EventBufferContext — receives each successfully mapped and deduplicated earthquake event as a PositionEvent

## Props

- api_url: URL of the USGS GeoJSON feed to poll
- poll_interval: how frequently the feed is fetched; once per minute is a sensible default
- min_magnitude: optional lower bound on magnitude; events below this threshold are silently dropped

## Role Methods

### event_sink
- receive_event: accepts a single PositionEvent and stores it for later querying

## Functionalities

### new
Stores the event sink, API URL, poll interval, and optional minimum magnitude. Initializes an empty set of seen event IDs for deduplication. Does not start polling during construction.

### start
Begins a continuous background loop. Waits for the poll interval to elapse before each fetch cycle. Retains the same configuration and deduplication state across iterations.

### fetch_events
Issues a GET request to the configured API URL. On a successful response, parses the GeoJSON feature collection and processes each feature. On failure, records a diagnostic error count and waits until the next interval.

### on_event_received
Attempts to parse the raw feature as an EarthquakeEvent. If the event ID has already been seen, discards it silently. Records each newly seen event ID for future deduplication. If a minimum magnitude is configured and the event's magnitude is below the threshold, discards the event silently. Maps latitude and longitude from the epicentre coordinates. Uses the event timestamp as occurred_at. Sets source to "earthquake". Forms the label from magnitude and place when both are available, using whichever is present otherwise. Delivers the resulting PositionEvent to the event sink.

## Scenarios

### duplicate event is deduplicated
Given: an already-seen earthquake event arrives a second time
When: on_event_received runs
Then: no PositionEvent is emitted and the duplicate is silently discarded

### magnitude below threshold is dropped
Given: a minimum magnitude of 3.0 and an event with magnitude 2.5
When: on_event_received runs
Then: the event is discarded

### successful poll with unique events
Given: a successful feed response containing five unique events with valid coordinates
When: fetch_events runs
Then: five PositionEvents with source "earthquake" can be delivered
