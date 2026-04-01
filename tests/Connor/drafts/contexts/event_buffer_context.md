# EventBufferContext

## Description

EventBufferContext is the in-memory store of recent PositionEvents.
It accepts events from whoever is playing the EventSource role, retains them for as long
as they fall within the configured TimeWindow, and discards them once they become stale.

The buffer makes no distinction between event sources. A PositionEvent from lightning, a
flight, an earthquake, AISStream, an ADS-B aircraft, or a Wikipedia edit is retained under
the same temporal rules. The buffer's job is purely temporal: it knows what happened
recently.

The buffer does not know about geography. It holds all events together and answers queries
that return either the full current population or a source-filtered subset; the
GridContext and AggregationContext are
responsible for interpreting those events spatially.

---

## Roles

- **clock**
  Provides the current moment in time.
  Used when deciding which events are within the window and which have expired.
  Fulfilled by a system clock or any equivalent time provider.

---

## Props

- **window**
  The TimeWindow defining how long an event is retained.
  Events older than the current time minus the window duration are evicted.
  The buffer does not impose an upper time bound at `clock.now()`: events whose
  `occurred_at` lies in the future are still retained.

---

## Role methods

### clock

- **now**
  Returns the current moment in time as a UTC timestamp.
  Must return the same kind of timestamp as PositionEvent.occurred_at so that the two
  can be compared directly when computing the staleness boundary.

---

## Internal state

The buffer maintains a private collection of PositionEvents in memory.
This collection is not directly accessible to outside collaborators.
It grows on each call to receive_event and shrinks during eviction.

---

## Functionalities

- **new(clock, window)**
  Constructs the buffer with the provided clock and the configured TimeWindow.
  Starts with an empty internal event collection.

- **window**
  Returns the TimeWindow currently configured on this buffer.
  Does not touch the event collection.
  Called by AggregationContext so it can stamp EventCounts with the active window
  and use the window's minutes value as the rate divisor.

- **receive_event(event)**
  Accepts an incoming PositionEvent and appends it to the buffer.
  Does not evict on every insert; eviction is driven by queries and by the periodic
  eviction cycle.

- **current_events**
  Returns all PositionEvents currently held in the buffer whose occurred_at is within
  the configured TimeWindow relative to the current moment.
  The staleness boundary is computed as the current time minus the window's minutes value.
  There is no freshness upper bound at the current moment: an event with
  `occurred_at > clock.now()` is included so long as it has not been evicted as stale.
  Evicts any stale events before returning the result, so the returned collection is
  always up to date.

- **current_events_for_source(source)**
  Returns the current PositionEvents for the requested source.
  First performs the same stale-event eviction as current_events.
  Then:
  - if `source` is absent, returns the same result as current_events,
  - if `source` is `lightning`, returns only events whose PositionEvent.source is `lightning`,
  - if `source` is `flight`, returns only events whose PositionEvent.source is `flight`,
  - if `source` is `earthquake`, returns only events whose PositionEvent.source is `earthquake`,
  - if `source` is `aisstream`, returns only events whose PositionEvent.source is `aisstream`,
  - if `source` is `adsb`, returns only events whose PositionEvent.source is `adsb`,
  - if `source` is `wiki`, returns only events whose PositionEvent.source is `wiki`.
  Filtering is based on PositionEvent.source, not on PositionEvent.label.

- **evict_stale**
  Removes all events whose occurred_at is older than the current time minus the
  window's minutes value.
  It does not remove future-dated events. After eviction, the buffer retains every
  event whose `occurred_at >= clock.now() - window.minutes()`, including timestamps
  later than `clock.now()`.
  This may be called on a schedule or triggered by current_events; either is acceptable.
  After eviction the buffer contains only events that would currently be returned by
  current_events.

---

## Acceptance examples

- Given an event with occurred_at five seconds ago and a window of five minutes, when
  current_events is called, then the event is included in the result.
- Given an event with occurred_at six minutes ago and a window of five minutes, when
  current_events is called, then the event is not included in the result.
- Given an event with occurred_at one minute in the future and a window of five minutes,
  when current_events is called, then the event is included in the result.
- Given the buffer contains two events, one stale and one fresh, when evict_stale runs,
  then only the fresh event remains.
- Given two receivers are both calling receive_event, when current_events is called,
  then events from both sources appear together in the result.
- Given the buffer contains lightning, flight, earthquake, AISStream, ADS-B, and wiki
  events, when current_events_for_source(lightning) is called, then only lightning
  events are returned.
