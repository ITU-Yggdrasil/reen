# FlightReceiverContext

## Description

FlightReceiverContext is the boundary adapter for the OpenSky Network REST API.
It polls the API at a regular interval, translates each returned FlightPosition into a
PositionEvent, and forwards those events into the wider system.

This context fulfils the EventSource role in exactly the same way as LightningReceiverContext.
The downstream EventBufferContext must not be able to distinguish which receiver is active.
Running both receivers simultaneously — so that the buffer receives both lightning and flight
events — must be a matter of configuration, not of changing any downstream code.

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
  The base URL of the OpenSky Network states endpoint.

- **api_token**
  An optional bearer token for authenticated OpenSky access.
  When present, each outbound HTTP request includes `Authorization: Bearer <token>`.
  When absent, requests are sent without an Authorization header.

- **poll_interval**
  How frequently the API should be queried. The default is a value that stays within
  OpenSky's anonymous rate limits.

- **bounding_box**
  An optional geographic region used to restrict the query. Defined inline as four
  decimal-degree values: min_latitude, max_latitude, min_longitude, max_longitude.
  When absent, the global feed is requested.

---

## Role methods

### event_sink

- **receive_event(event)**
  Accepts a single PositionEvent and stores it for later querying.
  The receiver calls this once per successfully mapped flight position.

---

## Functionalities

- **new(event_sink, api_url, api_token, poll_interval, bounding_box)**
  Constructs a FlightReceiverContext with the given role player and props.
  The event_sink is passed as a shared reference — the same EventBufferContext instance
  is also held by AggregationContext (and by any other active receiver). The application
  must not give up sole ownership of the buffer when passing it here.
  Stores the event_sink, api_url, api_token, poll_interval, and bounding_box. Does not start polling.
  Call start to begin the polling loop.

- **start**
  Begins the polling loop. Runs continuously in the background.
  On each iteration, waits for poll_interval to elapse, then calls fetch_positions.

- **fetch_positions**
  Issues a GET request to the configured api_url, passing the bounding_box if set.
  If api_token is present, includes it as an HTTP bearer token on that request.
  On a successful response, iterates over the returned flight state records and calls
  on_position_received for each one.
  On a failed or rate-limited response, records an error count for diagnostic purposes
  and waits until the next scheduled interval before retrying.

- **on_position_received(raw_position)**
  Attempts to parse the raw record as a FlightPosition.
  If the latitude or longitude is absent, discards the record silently.
  Otherwise, maps the flight position to a PositionEvent:
  - latitude and longitude are taken directly from the flight position,
  - occurred_at is taken from the flight position's timestamp,
  - PositionEvent.source is set to the canonical source value `flight`,
  - label is set to the callsign if present, otherwise to "flight".
  Passes the resulting PositionEvent to event_sink.receive_event.

---

## Acceptance examples

- Given a successful API response containing three positions with valid coordinates,
  when fetch_positions runs, then three PositionEvents are delivered to event_sink.
- Given a position record with no latitude, when on_position_received runs, then no
  event is delivered and the record is silently discarded.
- Given the API returns a rate-limit response, when fetch_positions runs, then no events
  are delivered and the error count increases by one, and the next attempt is deferred
  to the next scheduled interval.
