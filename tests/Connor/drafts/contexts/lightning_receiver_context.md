# LightningReceiverContext

## Description

LightningReceiverContext is the boundary adapter for the Blitzortung websocket feed.
It owns the connection to the Blitzortung network, keeps that connection alive, and
translates each incoming LightningStrike into a PositionEvent before forwarding it
into the wider system.

This context fulfils the EventSource role in the system. Any context that needs to
receive events must only know about the EventSource role — it must have no knowledge
that the underlying feed is Blitzortung, websocket-based, or lightning-specific.
Swapping this context out for the FlightReceiverContext, or running both side by side,
must require no changes to the downstream contexts.

---

## Roles

- **event_sink**
  The recipient of produced PositionEvents.
  Fulfilled by EventBufferContext.
  The receiver must not know the concrete type of the sink; it only calls the
  receive_event behaviour on whatever is playing this role.

---

## Props

- **websocket_url**
  The URL of the Blitzortung websocket endpoint to connect to.

- **reconnect_delay**
  How long to wait before attempting to reconnect after a dropped connection.
  This property is a duration value. In the application configuration shown by this
  repository it is sourced from `LIGHTNING_RECONNECT_DELAY_SECS`, so the configured unit
  at startup is seconds before being converted into an in-memory duration.

---

## Role methods

### event_sink

- **receive_event(event)**
  Accepts a single PositionEvent and stores it for later querying.
  The receiver calls this once per successfully mapped strike.

---

## Functionalities

- **new(event_sink, websocket_url, reconnect_delay)**
  Constructs a LightningReceiverContext with the given role player and props.
  The event_sink is passed as a shared reference — the same EventBufferContext instance
  is also held by AggregationContext (and by any other active receiver). The application
  must not give up sole ownership of the buffer when passing it here.
  Stores the event_sink, websocket_url, and reconnect_delay. Does not open the connection.
  Call start to begin receiving events.

- **start**
  Opens the websocket connection to the configured URL and begins reading messages.
  Runs continuously in the background.
  After the websocket handshake completes, sends the Blitzortung subscription message
  required to begin receiving strike traffic. The subscription message is a websocket
  text frame whose exact payload is:
  `{"a":111}`
  Registers this context as the active EventSource with any interested downstream
  contexts by virtue of being passed as the event_sink's upstream.

- **on_message_received(raw_message)**
  Called internally each time a message arrives on the socket.
  Normalises the incoming payload before parsing:
  - if the payload is already valid JSON, it is used as-is,
  - otherwise, if it matches the Blitzortung obfuscated websocket format, it is
    decoded into JSON first.
  Attempts to parse the resulting JSON as a LightningStrike.
  If parsing succeeds, maps the strike to a PositionEvent:
  - latitude and longitude are taken directly from the strike,
  - occurred_at is taken from the strike's timestamp,
  - PositionEvent.source is set to the canonical source value `lightning`,
  - label is set to "lightning".
  Passes the resulting PositionEvent to event_sink.receive_event.
  If parsing fails, discards the message, increments a parse-failure count, and logs
  the parse error together with the normalised payload for diagnostics.

- **on_connection_lost**
  Called internally when the websocket connection drops.
  Waits for the configured reconnect_delay and then allows the outer receive loop to
  establish a fresh websocket connection.
  Does not discard the existing event_sink or any configuration.

- **on_connection_established**
  Called internally once the websocket handshake completes successfully.
  Acts as the successful-connection hook. No reconnect back-off state is currently tracked.

---

## Parsing details

- Payload normalisation procedure:
  - If the incoming text is already valid JSON, use it unchanged.
  - Otherwise, attempt Blitzortung obfuscated decoding on the full text payload.
  - If the decoded result is valid JSON, use the decoded JSON.
  - Otherwise, normalisation fails and the original text payload is passed to strike
    parsing unchanged.
- Binary websocket frames are first interpreted as UTF-8 text. If UTF-8 decoding fails,
  normalisation fails.
- Failure accounting and logging rules:
  - For text frames, a normalisation failure is not counted separately; the original
    payload is still passed into strike parsing, and any resulting parse failure
    increments the parse-failure count by one.
  - For binary frames, if UTF-8 decoding or Blitzortung decoding fails before a text
    payload can be produced, the parse-failure count increments by one and a decode
    failure is logged; on_message_received is not invoked for that frame.
  - When on_message_received logs a parse failure, the logged payload is the
    normalised JSON string when normalisation succeeded, otherwise the original raw
    text payload.
- Blitzortung obfuscated decoding is defined as follows:
  - Read the payload as a sequence of Unicode scalar values.
  - Emit the first character unchanged.
  - Maintain an initially empty dictionary of strings.
  - Maintain `current_prefix` as the most recently decoded entry. Initially it is the
    same single-character string as `previous_entry`.
  - For each subsequent character:
    - if its code point is less than `256`, treat that character itself as the next
      decoded entry;
    - otherwise, subtract `256` from the code point and use the result as a zero-based
      dictionary index;
    - if that dictionary index does not yet exist, derive the entry as
      `previous_entry + first_character_of_current_prefix`.
  - Append the decoded entry to the output.
  - Append `previous_entry + first_character_of_decoded_entry` to the dictionary.
  - Update both `previous_entry` and `current_prefix` to the decoded entry, then continue
    until the payload is exhausted.
- Accepts latitude from `lat` or `latitude`.
- Accepts longitude from `lon`, `lng`, or `longitude`.
- Accepts timestamp from `time`, `timestamp`, or `ts`.
- Accepts numeric fields either as JSON numbers or as numeric strings.
- Accepts timestamps at second, millisecond, microsecond, or nanosecond precision.

---

## Acceptance examples

- Given a valid strike message arrives, when on_message_received runs, then a PositionEvent
  with label "lightning" is delivered to event_sink.
- Given an obfuscated Blitzortung text or binary payload arrives, when on_message_received
  runs, then the payload is decoded to JSON before strike parsing is attempted.
- Given a malformed message arrives, when on_message_received runs, then no event is
  delivered, the parse-failure count increases by one, and a warning is logged.
- Given a text payload that cannot be normalised into JSON, when on_message_received
  runs, then the original text is still passed to strike parsing and any resulting
  parse failure increases the parse-failure count by one.
- Given a binary payload that cannot be decoded into text, when it is received from
  the websocket, then the parse-failure count increases by one and a decode failure is
  logged without invoking on_message_received.
- Given the connection drops, when on_connection_lost runs, then a reconnection attempt
  is made after reconnect_delay has elapsed.
