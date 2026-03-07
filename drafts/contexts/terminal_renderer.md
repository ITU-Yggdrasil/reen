# Terminal Renderer

## Description

This context is a render used to render the current frame for a game. This rendered specifically renders the game frame as ascii-art in the terminal.

## Functionality

- **render** 
  Renders based on a coordinate system with (0,0) in the bottom left corner and (width - 1, height - 1) in the upper right corner
  - input:
    - **board** a two dimensional array of chars representing the current board. The array represents the cells in the grid. board[x][y] matches the coordinate (x,y).
    - **score** The current score of the game

  - Render the current game state to the terminal:
     - Rendering is in-place: clear the terminal and move cursor to the top-left before drawing each frame.
     - for each cell in the board simply print the character, start a the line from (0, height - 1) to (widht - 1, height - 1) i.e. the top most and work the way down to (0,0) to (width - 1, 0) i.e. the bottom most line.
     - Each rendered line must start at column 0 (use carriage return semantics so rows do not drift right in raw mode).
     - Display the score below the board to the left