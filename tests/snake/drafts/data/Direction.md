# Direction

## Description
A value type representing movement direction for the Snake. An enum with four different values
 - UP
 - DOWN
 - LEFT
 - RIGHT


## Invariants
- Opposites: `UP <-> DOWN`, `LEFT <-> RIGHT`

## Functionality
- **is_opposite** -> takes a nother direction an returns true if self and other are opposites