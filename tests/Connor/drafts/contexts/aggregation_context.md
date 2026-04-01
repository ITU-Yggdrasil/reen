# AggregationContext

## Description

AggregationContext is the analytical layer that translates a population of recent
PositionEvents into per-cell EventCounts and EventRates.

It does not store events itself and it does not own the grid. Both of those concerns
belong to collaborators it works with via roles. On each request, it asks the buffer
for the current event population, asks the grid to assign each event to a cell, and
then tallies the results.

AggregationContext is stateless between calls: every invocation of produce_counts or
produce_rates recomputes from scratch against the current state of the buffer.

---

## Roles

- **buffer**
  Provides the current population of recent PositionEvents.
  Fulfilled by EventBufferContext.

- **grid**
  Provides the spatial organisation of the world into GeographicCells.
  Fulfilled by GridContext.

---

## Role methods

### buffer

- **current_events**
  Returns all PositionEvents currently within the configured time window.

- **window**
  Returns the TimeWindow currently configured on the buffer.
  Used by produce_counts to stamp each EventCount with the active window, and by
  produce_rates to obtain the window's minutes value as the divisor for rate calculation.

### grid

- **cell_for(latitude, longitude)**
  Returns the GeographicCell that contains the given coordinate.

- **all_cells**
  Returns the complete list of GeographicCells in the grid.

---

## Functionalities

- **new(buffer, grid)**
  Constructs the context with the provided buffer and grid role players.
  The buffer is passed as a shared reference (the same instance is also held by the
  receiver contexts), so the application must not hand over sole ownership of the buffer
  here. Performs no computation at construction time.

- **produce_counts**
  Returns an EventCount for every cell in the grid.
  Steps:
  1. Calls buffer.window to obtain the active TimeWindow.
  2. Calls buffer.current_events to get the live event population.
  3. For each event, calls grid.cell_for with the event's latitude and longitude to
     determine which cell it belongs to.
  4. Tallies the number of events per cell.
  5. Calls grid.all_cells to obtain the full cell list.
  6. Returns one EventCount per cell. Cells with no matching events receive a count
     of zero — no cell is omitted from the result.
  The window field of each EventCount is set to the TimeWindow obtained in step 1.

- **produce_rates**
  Returns an EventRate for every cell in the grid.
  Calls produce_counts internally. For each EventCount, divides the count by the
  window's minutes value to derive the rate.
  Because TimeWindow.minutes is defined as a positive whole-number `i32` with minimum
  value `1`, this division is always well-defined for valid inputs and cannot divide by zero.
  Returns one EventRate per cell in the same exhaustive fashion as produce_counts.
  The window field of each EventRate is the same TimeWindow carried by its EventCount.

---

## Acceptance examples

- Given the buffer holds three events all in the same cell, when produce_counts runs,
  then that cell has a count of three and all other cells have a count of zero.
- Given a window of five minutes and a cell count of ten, when produce_rates runs,
  then that cell has an events_per_minute of two.
- Given the buffer is empty, when produce_counts runs, then all cells have a count of
  zero and the total number of EventCounts equals the number of cells in the grid.
