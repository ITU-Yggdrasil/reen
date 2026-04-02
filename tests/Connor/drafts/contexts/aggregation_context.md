# AggregationContext

## Purpose

AggregationContext is the analytical layer that translates a population of recent
PositionEvents into per-cell EventCounts and EventRates.

It does not store events itself and it does not own the grid. Both of those concerns
belong to collaborators it works with via roles. On each request, it asks the buffer
for the current event population, asks the grid to assign each event to a cell, and
then tallies the results.

AggregationContext is stateless between calls: every invocation of `produce_counts` or
`produce_rates` recomputes from scratch against the current state of the buffer.

## Role Players

| Role player | Why involved | Expected behaviour |
|---|---|---|
| buffer | Supplies the current population of recent PositionEvents | Provides current events and the active TimeWindow |
| grid | Supplies the geographic cell layout | Maps coordinates to cells and enumerates all cells |

## Role Methods

### buffer

- **current_events** Returns all PositionEvents currently within the configured time window.
- **window** Returns the active TimeWindow used to stamp counts and derive rates.

### grid

- **cell_for(latitude, longitude)** Returns the GeographicCell that contains the given coordinate.
- **all_cells** Returns the complete list of GeographicCells in the grid.

## Props

## Functionalities

### new

| Started by | Uses | Result |
|---|---|---|
| application startup | buffer, grid | aggregation context is constructed |

Rules:
- Stores the provided buffer and grid role players.
- The buffer is passed as a shared reference.
- Performs no computation at construction time.

| Given | When | Then |
|---|---|---|
| a buffer and grid are available | new is called | an AggregationContext is returned |

### produce_counts

| Started by | Uses | Result |
|---|---|---|
| caller requesting counts | buffer, grid | one EventCount per grid cell is returned |

Rules:
- Calls `buffer.window` to obtain the active TimeWindow.
- Calls `buffer.current_events` to obtain the live event population.
- Calls `grid.cell_for` for each event to determine its cell.
- Tallies the number of events per cell.
- Calls `grid.all_cells` to obtain the full cell list.
- Returns one EventCount per cell.
- Cells with no matching events receive a count of zero.
- The `window` field of each EventCount is set to the TimeWindow obtained from the buffer.

| Given | When | Then |
|---|---|---|
| the buffer holds three events in one cell | produce_counts runs | that cell has a count of three and all other cells have zero |

### produce_rates

| Started by | Uses | Result |
|---|---|---|
| caller requesting rates | produce_counts | one EventRate per grid cell is returned |

Rules:
- Calls `produce_counts` internally.
- Divides each EventCount by the window's minutes value to derive the rate.
- Because TimeWindow minutes are always at least `1`, valid inputs do not divide by zero.
- Returns one EventRate per cell in the same exhaustive fashion as `produce_counts`.
- Carries the same TimeWindow from each EventCount into the resulting EventRate.

| Given | When | Then |
|---|---|---|
| a window of five minutes and a cell count of ten | produce_rates runs | that cell has an events_per_minute value of two |
