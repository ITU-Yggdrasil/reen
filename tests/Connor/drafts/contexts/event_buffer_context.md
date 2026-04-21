# Context: EventBufferContext

## Purpose

EventBuffer is the in-memory store of recent PositionEvents. It accepts events from any event source, retains them within the configured TimeWindow, and discards them once they become stale. The buffer makes no distinction between event sources — a PositionEvent from lightning, flight, earthquake, AISStream, ADS-B, or Wikipedia is retained under the same temporal rules. The buffer does not know about geography; it holds all events together and answers queries for the full current population or a source-filtered subset.

## Role Players

- clock: Clock — supplies the current moment in time as a UTC timestamp for staleness checks

## Props

- window: the TimeWindow defining how long an event is retained; future-dated events are still retained

## Role Methods

### clock
- now: returns the current moment in time as a UTC timestamp

## Functionalities

### new
Constructs the buffer with the provided clock and TimeWindow. Starts with an empty internal event collection.

### window
Returns the TimeWindow currently configured on the buffer. Does not modify the event collection.

### receive_event
Accepts an incoming PositionEvent and adds it to the buffer. Does not evict stale events on every insert; eviction is driven by queries or explicit eviction.

### current_events
Returns all PositionEvents whose occurred_at is within the configured TimeWindow relative to the current moment. Computes the staleness boundary as the current time minus the window's duration in minutes. Does not impose an upper freshness bound at the current time — future-dated events are included as long as they are not stale. Evicts stale events before returning the result.

### current_events_for_source
Performs the same stale-event eviction as current_events. If the source filter is absent, returns the same result as current_events. If the source is "lightning", "flight", "earthquake", "aisstream", "adsb", or "wiki", returns only events whose source matches that value. Filtering is based on the event source, not the label.

### evict_stale
Removes all events whose occurred_at is older than the current time minus the window's duration in minutes. Does not remove future-dated events. After eviction, the buffer contains only events that would currently be returned by current_events.

## Scenarios

### buffer starts empty
Given: a clock and a five-minute window
When: new is called
Then: the buffer starts empty

### recent event is included
Given: an event from five seconds ago and a five-minute window
When: current_events is called
Then: the event is included

### stale event is evicted
Given: the buffer contains one stale event and one fresh event
When: evict_stale runs
Then: only the fresh event remains

### source filter returns only matching events
Given: the buffer contains lightning, flight, earthquake, AISStream, ADS-B, and wiki events
When: current_events_for_source is called with source "lightning"
Then: only lightning events are returned

### receive adds event to buffer
Given: a fresh PositionEvent
When: receive_event is called
Then: the event becomes part of the buffer
