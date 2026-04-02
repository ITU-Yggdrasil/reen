# EventBufferContext

## Purpose

EventBufferContext is the in-memory store of recent PositionEvents.
It accepts events from whoever is playing the EventSource role, retains them for as long
as they fall within the configured TimeWindow, and discards them once they become stale.

The buffer makes no distinction between event sources. A PositionEvent from lightning, a
flight, an earthquake, AISStream, an ADS-B aircraft, or a Wikipedia edit is retained under
the same temporal rules. The buffer's job is purely temporal: it knows what happened
recently.

The buffer does not know about geography. It holds all events together and answers queries
that return either the full current population or a source-filtered subset; GridContext and
AggregationContext are responsible for interpreting those events spatially.

## Role Players

| Role player | Why involved | Expected behaviour |
|---|---|---|
| clock | Supplies the current moment in time | Returns a UTC timestamp for staleness checks |

## Role Methods

### clock

- **now** Returns the current moment in time as a UTC timestamp compatible with `PositionEvent.occurred_at`.

## Props

| Prop | Meaning | Notes |
|---|---|---|
| window | TimeWindow defining how long an event is retained | Future-dated events are still retained |

## Functionalities

### new

| Started by | Uses | Result |
|---|---|---|
| application startup | clock, window | event buffer is constructed |

Rules:
- Constructs the buffer with the provided clock and TimeWindow.
- Starts with an empty internal event collection.

| Given | When | Then |
|---|---|---|
| a clock and five-minute window | new is called | the buffer starts empty |

### window

| Started by | Uses | Result |
|---|---|---|
| aggregation logic | window | active TimeWindow is returned |

Rules:
- Returns the TimeWindow currently configured on the buffer.
- Does not modify the event collection.

| Given | When | Then |
|---|---|---|
| a configured buffer | window is called | the configured TimeWindow is returned |

### receive_event

| Started by | Uses | Result |
|---|---|---|
| receiver context | event | event is appended to the buffer |

Rules:
- Accepts an incoming PositionEvent and appends it to the buffer.
- Does not evict on every insert.
- Eviction is driven by queries or explicit eviction.

| Given | When | Then |
|---|---|---|
| a fresh PositionEvent | receive_event is called | the event becomes part of the buffer |

### current_events

| Started by | Uses | Result |
|---|---|---|
| caller requesting live events | clock, window, stored events | current live events are returned |

Rules:
- Returns all PositionEvents whose `occurred_at` is within the configured TimeWindow relative to the current moment.
- Computes the staleness boundary as `clock.now() - window.minutes()`.
- Does not impose an upper freshness bound at `clock.now()`.
- Includes future-dated events so long as they are not stale.
- Evicts stale events before returning the result.

| Given | When | Then |
|---|---|---|
| an event from five seconds ago and a five-minute window | current_events is called | the event is included |

### current_events_for_source

| Started by | Uses | Result |
|---|---|---|
| caller requesting filtered live events | current_events, source | source-filtered events are returned |

Rules:
- Performs the same stale-event eviction as `current_events`.
- If `source` is absent, returns the same result as `current_events`.
- If `source` is `lightning`, `flight`, `earthquake`, `aisstream`, `adsb`, or `wiki`, returns only events whose `PositionEvent.source` matches that value.
- Filtering is based on `PositionEvent.source`, not on `PositionEvent.label`.

| Given | When | Then |
|---|---|---|
| the buffer contains lightning, flight, earthquake, AISStream, ADS-B, and wiki events | current_events_for_source(lightning) is called | only lightning events are returned |

### evict_stale

| Started by | Uses | Result |
|---|---|---|
| scheduler or query path | clock, window, stored events | stale events are removed |

Rules:
- Removes all events whose `occurred_at` is older than `clock.now() - window.minutes()`.
- Does not remove future-dated events.
- After eviction, the buffer contains only events that would currently be returned by `current_events`.

| Given | When | Then |
|---|---|---|
| the buffer contains one stale event and one fresh event | evict_stale runs | only the fresh event remains |

## Notes

The buffer maintains a private collection of PositionEvents in memory. That collection is
not directly accessible to outside collaborators. It grows on each call to `receive_event`
and shrinks during eviction.
