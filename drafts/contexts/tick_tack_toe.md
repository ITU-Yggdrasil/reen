# tick_tack_toe

# Description
A two-player game played on a 3x3 grid.
Players take turns placing their mark on empty cells until one player has three in a row or all cells are filled.

# Responsibilities
- Track the current board state.
- Track which player's turn it is.
- Apply a player's move if the chosen cell is empty.
- Detect win (three in a row), draw, or ongoing game.

# Inputs
- current_player: "X" or "O".
- board: 3x3 grid where each cell is "X", "O", or empty.
- move: position chosen by the current player (row and column index 0–2).

# Outputs
- updated_board: the board after applying the move.
- next_player: which player should move next ("X" or "O").
- status: "ongoing", "x_wins", "o_wins", or "draw".

# Main Flow
1. Receive the current board, current_player, and the chosen move.
2. If the chosen cell is not empty, treat this as an invalid move (behavior can be refined later).
3. Place current_player's mark in the chosen cell.
4. Check if current_player now has three in a row (horizontal, vertical, or diagonal).
   - If yes, set status to "<player>_wins".
5. If no winner and the board is full, set status to "draw".
6. If no winner and the board is not full, set status to "ongoing" and switch next_player.

# Edge Cases
- Moves outside the valid index range (0–2).
- Attempting to play on a non-empty cell.