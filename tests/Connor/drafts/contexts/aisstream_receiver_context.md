# AISStreamReceiverContext

## Description

AISStreamReceiverContext is the wrapper that turns raw AISStream position messages into
feed-agnostic PositionEvents for the rest of the system.

It does not own the websocket connection, authentication, or reconnect behaviour itself.
Those concerns belong to the collaborator playing the `aisstream` role. This context asks
that collaborator for an AISStream subscription limited to the included position-message
types, receives the resulting boundary records, and forwards mapped PositionEvents into the
wider system.

This context fulfils the EventSource role in exactly the same way as every other receiver.
The downstream EventBufferContext must not be able to distinguish which receiver is active.
Running this context alongside any other receiver must be a matter of configuration, not of
changing any downstream code.

---

## Roles

- **aisstream**
  The upstream collaborator that owns AISStream communication.
  Fulfilled by a future AISStreamContext derived from `drafts/apis/aisstream.md`.
  This role is responsible for opening the websocket, authenticating with the API key,
  maintaining the connection, and delivering only subscribed AISStream messages to this
  receiver.

- **event_sink**
  The recipient of produced PositionEvents.
  Fulfilled by EventBufferContext.
  The receiver must not know the concrete type of the sink; it only calls the
  `receive_event` behaviour on whatever is playing this role.

---

## Props

- **bounding_box**
  An optional geographic region used to restrict the AISStream subscription. Defined inline
  as four decimal-degree values: `min_latitude`, `max_latitude`, `min_longitude`,
  `max_longitude`.
  When absent, the global AISStream feed is subscribed to.

---

## Role methods

### aisstream

- **start_position_subscription(message_sink, bounding_box, filter_message_types)**
  Starts or refreshes an AISStream subscription and delivers matching raw messages to the
  supplied `message_sink`.
  `filter_message_types` is the positive filter from `drafts/apis/aisstream.md` and, for
  this receiver, must be exactly:
  - `PositionReport`
  - `StandardClassBPositionReport`

### event_sink

- **receive_event(event)**
  Accepts a single PositionEvent and stores it for later querying.
  The receiver calls this once per successfully mapped AISStream position.

---

## Functionalities

- **new(aisstream, event_sink, bounding_box)**
  Constructs an AISStreamReceiverContext with the given role players and props.
  The `event_sink` is passed as a shared reference - the same EventBufferContext instance
  is also held by AggregationContext (and by any other active receiver). The application
  must not give up sole ownership of the buffer when passing it here.
  Stores all provided values. Does not start the upstream subscription.
  Call `start` to begin receiving events.

- **start**
  Asks the collaborator playing the `aisstream` role to start a position subscription for
  this receiver.
  The subscription must use the configured `bounding_box` and the fixed positive filter:
  - `PositionReport`
  - `StandardClassBPositionReport`
  Raw upstream messages outside those two AISStream message families are not delivered to
  this receiver.

- **receive_aisstream_message(raw_message)**
  Called by the collaborator playing the `aisstream` role each time a subscribed message is
  available.
  Attempts to parse `raw_message` as an AISStreamPositionMessage.
  If the latitude or longitude is absent or carries the AIS default invalid value
  (91.0 degrees latitude or 181.0 degrees longitude), discards the record silently.
  Otherwise, maps the boundary record to a PositionEvent:
  - latitude and longitude are taken directly from the AISStream position message,
  - occurred_at is taken from the boundary record's `observed_at`,
  - source is set to `aisstream`,
  - label is set to the vessel name if present and non-blank, otherwise to the MMSI
    formatted as a string.
  Passes the resulting PositionEvent to `event_sink.receive_event`.
  If parsing fails, discards the message silently and records a parse-failure count for
  diagnostic purposes.

---

## Parsing details

- The positive filter is intentionally narrow: only `PositionReport` and
  `StandardClassBPositionReport` are included because they are sufficient to plot moving
  vessel dots on the map.
- AISStream wraps each payload in an envelope containing `MessageType`, `Message`, and
  provider-specific `MetaData`.
- The message-body field names differ by `MessageType`, but both included message families
  expose `UserID`, `Latitude`, `Longitude`, and `Timestamp`.
- The AIS `Timestamp` field in these message families is not a full UTC instant; it is only
  the reported second within the current UTC minute. The receiver therefore uses the
  boundary record's `observed_at` field as the event timestamp.
- Vessel name may be absent because AISStream message metadata is provider-specific and the
  included message families are position reports rather than static-data reports.

---

## Acceptance examples

- Given AISStream delivers a `PositionReport` with valid coordinates and a vessel name,
  when `receive_aisstream_message` runs, then a PositionEvent with source `aisstream` and
  the vessel name as label is delivered to `event_sink`.
- Given AISStream delivers a `StandardClassBPositionReport` with latitude 91.0, when
  `receive_aisstream_message` runs, then no event is delivered and the record is silently
  discarded.
- Given AISStream delivers a subscribed position message with no vessel name, when
  `receive_aisstream_message` runs, then a PositionEvent with the MMSI as label is
  delivered to `event_sink`.
- Given `start` is called with a configured bounding box, when the receiver requests the
  subscription from `aisstream`, then only `PositionReport` and
  `StandardClassBPositionReport` are requested for that region.
