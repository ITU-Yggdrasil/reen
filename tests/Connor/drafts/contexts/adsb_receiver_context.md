# AdsbReceiverContext

## Description

AdsbReceiverContext is the boundary adapter for the ADS-B Exchange REST API. It polls the
API at a regular interval, translates each returned AircraftState into a PositionEvent, and
forwards those events into the wider system.

This context fulfils the EventSource role in exactly the same way as every other receiver.
The downstream EventBufferContext must not be able to distinguish which receiver is active.
Running this context alongside any other receiver must be a matter of configuration, not of
changing any downstream code.

ADS-B Exchange provides near-real-time positions of aircraft worldwide, aggregated from a
volunteer network of ground-based receivers. Each response is a snapshot of all currently
tracked aircraft, not a stream of individual updates. The receiver polls on a fixed interval
and emits one PositionEvent per aircraft per poll cycle, provided the aircraft has a valid
position fix.

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
  The base URL of the ADS-B Exchange aircraft endpoint. When a bounding_box is provided,
  the receiver constructs a region-scoped URL (e.g., the lat/lon/dist form); otherwise
  it requests the global snapshot.

- **api_key**
  The API key required to authenticate with ADS-B Exchange. Sent as a request header.

- **poll_interval**
  How frequently the API should be queried. ADS-B Exchange data updates approximately
  every second server-side, but rate limits apply at the API tier; a sensible default
  aligns with the permitted request rate for the configured API key tier.

- **bounding_box**
  An optional geographic region used to restrict the query. Defined inline as four
  decimal-degree values: min_latitude, max_latitude, min_longitude, max_longitude.
  When absent, the global aircraft snapshot is requested.

---

## Role methods

### event_sink

- **receive_event(event)**
  Accepts a single PositionEvent and stores it for later querying.
  The receiver calls this once per successfully mapped aircraft state.

---

## Functionalities

- **new(event_sink, api_url, api_key, poll_interval, bounding_box)**
  Constructs an AdsbReceiverContext with the given role player and props.
  The event_sink is passed as a shared reference — the same EventBufferContext instance
  is also held by AggregationContext (and by any other active receiver). The application
  must not give up sole ownership of the buffer when passing it here.
  Stores all provided values. Does not start polling.
  Call start to begin the polling loop.

- **start**
  Begins the polling loop. Runs continuously in the background.
  On each iteration, waits for poll_interval to elapse, then calls fetch_aircraft.

- **fetch_aircraft**
  Issues a GET request to the configured api_url, passing the bounding_box if set and
  the api_key in the request header.
  On a successful response, iterates over the returned aircraft records and calls
  on_aircraft_received for each one.
  On a failed or rate-limited response, records an error count for diagnostic purposes
  and waits until the next scheduled interval before retrying.

- **on_aircraft_received(raw_state)**
  Attempts to parse the raw record as an AircraftState.
  If the latitude or longitude is absent, discards the record silently.
  Otherwise, maps the aircraft state to a PositionEvent:
  - latitude and longitude are taken directly from the aircraft state,
  - occurred_at is taken from the state's timestamp,
  - source is set to "adsb",
  - label is set to the callsign if present and non-blank, otherwise to the ICAO hex code.
  Passes the resulting PositionEvent to event_sink.receive_event.

---

## Parsing details

- The ADS-B Exchange API returns aircraft records in an `ac` array within a JSON object.
- Altitude may be numeric or the string `"ground"`.
- Latitude and longitude are floating-point numbers; absent means no field or null.
- Timestamp is typically a Unix epoch integer in seconds; the `now` field in the response
  envelope gives the snapshot time and can be used as a fallback when per-aircraft time
  is absent.
- Callsign may be an empty string rather than absent; both must be treated as missing.

---

## Acceptance examples

- Given a successful response containing four aircraft with valid coordinates, when
  fetch_aircraft runs, then four PositionEvents are delivered to event_sink.
- Given an aircraft record with no latitude, when on_aircraft_received runs, then no
  event is delivered and the record is silently discarded.
- Given an aircraft with a blank callsign, when on_aircraft_received runs, then the
  resulting PositionEvent uses the ICAO hex code as the label.
- Given the API returns a rate-limit response, when fetch_aircraft runs, then no events
  are delivered, the error count increases by one, and the next attempt is deferred to
  the next scheduled interval.
