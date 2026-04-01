# EarthquakeReceiverContext

## Description

EarthquakeReceiverContext is the boundary adapter for the USGS Earthquake Hazards GeoJSON
feed. It polls the USGS API at a regular interval, translates each returned EarthquakeEvent
into a PositionEvent, and forwards those events into the wider system.

This context fulfils the EventSource role in exactly the same way as every other receiver.
The downstream EventBufferContext must not be able to distinguish which receiver is active.
Running this context alongside any other receiver must be a matter of configuration, not of
changing any downstream code.

The USGS feed provides global earthquake detections updated approximately every minute. Each
response returns all events that occurred within a recent time window (typically the past
hour). Because the same event will appear in many successive responses, the receiver is
responsible for deduplicating by event ID before forwarding to the buffer.

---

## Roles

- **event_sink**
  The recipient of produced PositionEvents.
  Fulfilled by EventBufferContext.
  The receiver must not know the concrete type of the sink; it only calls the
  receive_event behaviour on whatever is playing this role.

---

## Props

- **api_url**
  The URL of the USGS GeoJSON feed to poll.
  The feed granularity (all earthquakes vs. significant only, past hour vs. past day) is
  encoded in the URL and chosen at configuration time.

- **poll_interval**
  How frequently the feed should be fetched. The USGS feed is updated approximately once
  per minute, so polling more frequently than that is wasteful; once per minute is a
  sensible default.

- **min_magnitude**
  Optional lower bound on magnitude. Events below this threshold are silently dropped
  after fetch. When absent, all events in the feed are forwarded.

---

## Role methods

### event_sink

- **receive_event(event)**
  Accepts a single PositionEvent and stores it for later querying.
  The receiver calls this once per successfully mapped and deduplicated earthquake event.

---

## Functionalities

- **new(event_sink, api_url, poll_interval, min_magnitude)**
  Constructs an EarthquakeReceiverContext with the given role player and props.
  The event_sink is passed as a shared reference — the same EventBufferContext instance
  is also held by AggregationContext (and by any other active receiver). The application
  must not give up sole ownership of the buffer when passing it here.
  Stores the event_sink, api_url, poll_interval, and min_magnitude. Does not start polling.
  Initialises an empty set of seen event IDs for deduplication.
  Call start to begin the polling loop.

- **start**
  Begins the polling loop. Runs continuously in the background.
  On each iteration, waits for poll_interval to elapse, then calls fetch_events.

- **fetch_events**
  Issues a GET request to the configured api_url.
  On a successful response, parses the GeoJSON FeatureCollection and calls
  on_event_received for each feature.
  On a failed response, records an error count for diagnostic purposes and waits until
  the next scheduled interval before retrying.

- **on_event_received(raw_event)**
  Attempts to parse the raw GeoJSON feature as an EarthquakeEvent.
  If the event ID has already been seen, discards it silently (deduplication).
  Records the event ID as seen.
  If min_magnitude is configured and the event's magnitude is below the threshold,
  discards the event silently.
  Otherwise, maps the earthquake event to a PositionEvent:
  - latitude and longitude are taken from the event's epicentre coordinates,
  - occurred_at is taken from the event's timestamp,
  - source is set to "earthquake",
  - label is formed from the magnitude and place (e.g., "M3.1 — 12km NNE of Ridgecrest CA");
    if either is absent, the available piece is used alone.
  Passes the resulting PositionEvent to event_sink.receive_event.

---

## Parsing details

- The USGS feed uses GeoJSON coordinate order: [longitude, latitude, depth].
- Timestamps are provided as milliseconds since Unix epoch (integer).
- Magnitude is a floating-point number in the `mag` property; may be null for very recent events.
- Event ID is in the `id` field of each GeoJSON feature (string).
- The `place` property is a string or null.

---

## Acceptance examples

- Given a successful feed response containing five events, all with unique IDs and valid
  coordinates, when fetch_events runs, then five PositionEvents are delivered to event_sink.
- Given the same event appears in two successive responses, when on_event_received runs
  for the second occurrence, then no event is delivered and the duplicate is silently discarded.
- Given an event with magnitude below min_magnitude, when on_event_received runs, then
  no event is delivered and the record is silently discarded.
- Given the API returns an error response, when fetch_events runs, then no events are
  delivered, the error count increases by one, and the next attempt is deferred to the
  next scheduled interval.
