# Direction

## Description
A value type representing movement direction for the Snake. An enum with four different values
 - Up
 - Down
 - Left
 - Right


## Invariants
- Opposites: `Up <-> Down`, `Left <-> Right`

## Functionality
- **is_opposite** -> takes a nother direction an returns true if self and other are opposites