# Board

## Description

A rectangular playfield that defines valid positions and acts as the primary obstacle boundary (walls/bounds) for Snake.

the coordinate system is 0 based starting in the lower left corner (0,0) extending to the upper right (width - 1, height -1). cells where either x or y is zero or where x is width - 1 or y is height - 1 are considered to be the boundary and not part of the playing field. i.e. the are walls/obstacles

## Fields
- **width**
- **height**

## Invariants
- `width > 0` and `height > 0`