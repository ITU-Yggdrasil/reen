# Context: AggregationContext

## Purpose

Aggregation is the analytical layer that translates a population of recent PositionEvents into per-cell EventCounts and EventRates. It does not store events itself and it does not own the grid. Both of those concerns belong to collaborators it works with via roles. On each request, it asks the buffer for the current event population, asks the grid to assign each event to a cell, and tallies the results. Aggregation is stateless between calls: every invocation recomputes from scratch against the current state of the buffer.

## Role Players

- buffer: EventBufferContext — supplies the current population of recent PositionEvents and the active TimeWindow
- grid: GridContext — maps coordinates to cells and enumerates all cells

## Props

## Role Methods

### buffer
- current_events: returns all PositionEvents currently within the configured time window
- window: returns the active TimeWindow used to stamp counts and derive rates

### grid
- cell_for: returns the GeographicCell that contains a given latitude and longitude
- all_cells: returns the complete list of GeographicCells in the grid

## Functionalities

### new
Stores the provided buffer and grid role players. Performs no computation at construction time.

### produce_counts
Asks the buffer for the active TimeWindow and the current event population. For each event, asks the grid which cell it belongs to, then tallies the number of events per cell. Returns one EventCount per grid cell. Cells with no matching events receive a count of zero. Each EventCount carries the TimeWindow obtained from the buffer.

### produce_rates
Calls produce_counts internally. Divides each EventCount by the window's duration in minutes to derive the rate. Because the TimeWindow is always at least one minute, valid inputs do not divide by zero. Returns one EventRate per cell in the same exhaustive fashion as produce_counts, carrying the same TimeWindow.

## Scenarios

### three events in one cell
Given: the buffer holds three events in one cell
When: produce_counts is called
Then: that cell has a count of three and all other cells have zero

### rate from count and window
Given: a window of five minutes and a cell count of ten
When: produce_rates is called
Then: that cell has an events_per_minute value of two

### empty buffer produces zero counts
Given: the buffer holds no events
When: produce_counts is called
Then: every cell has a count of zero
