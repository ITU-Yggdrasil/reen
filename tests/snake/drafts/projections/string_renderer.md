# Projection: StringRenderer

## Purpose

StringRenderer turns one board picture and a score into plain text. It is the canonical formatter for the visible game frame.

## Source

- Board

## Queries

### new
Stores the board as the source to query for on-screen symbols. No other collaborators are fetched.

### render
Produces a complete text frame from the board and a given score.

Iterates rows from top to bottom. Within each row, iterates columns from left to right, collecting the symbol at each coordinate. Each row is followed by a newline. After the last row, a score line in the format "Score: <score>" followed by a newline is included. Returns the complete text.

## Scenarios

### render a board with a score
Given: a board picture and a score of 12
When: render is called
Then: the returned text contains each row's symbols followed by newlines, ending with "Score: 12" and a newline

### empty interior shows spaces
Given: a 5 by 5 board where all interior cells are spaces
When: render is called
Then: the interior rows contain only spaces between the wall symbols
