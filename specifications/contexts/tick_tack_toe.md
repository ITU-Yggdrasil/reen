# Tic-Tac-Toe Specification

## Description
A two-player game played on a 3x3 grid. Players alternate turns placing their mark ("X" or "O") on empty cells. The game concludes when one player achieves three of their marks in a row (horizontally, vertically, or diagonally) or all cells are filled without a winner.

---

## Responsibilities
1. **Track the current board state**: Maintain a 3x3 grid representing the game board.
2. **Track the current player**: Identify which player ("X" or "O") is expected to make the next move.
3. **Apply a player's move**: Update the board state if the chosen cell is empty.
4. **Determine game status**: Detect whether the game is ongoing, won by a player, or a draw.

---

## Inputs
| Input            | Description                                                                                     |
|------------------|-------------------------------------------------------------------------------------------------|
| `current_player` | The player making the move. Valid values: `"X"` or `"O"`.                                      |
| `board`          | A 3x3 grid representing the current state of the game. Each cell is `"X"`, `"O"`, or empty.    |
| `move`           | The position chosen by the `current_player`. Specified as a row and column index (0–2).        |

---

## Outputs
| Output           | Description                                                                                     |
|------------------|-------------------------------------------------------------------------------------------------|
| `updated_board`  | The board state after applying the move.                                                        |
| `next_player`    | The player expected to move next. Valid values: `"X"` or `"O"`.                                |
| `status`         | The result of the move. Valid values: `"ongoing"`, `"x_wins"`, `"o_wins"`, or `"draw"`.        |

---

## Main Flow
1. **Receive inputs**: Accept the `current_player`, `board`, and `move`.
2. **Validate move**:
   - If the chosen cell is **not empty**, treat the move as invalid.
   - If the move is invalid, the behavior is unspecified.
3. **Apply move**: Place the `current_player`'s mark in the chosen cell.
4. **Check for a win**:
   - Determine if the `current_player` has three marks in a row (horizontally, vertically, or diagonally).
   - If a win is detected, set `status` to `"x_wins"` or `"o_wins"` (matching the `current_player`).
5. **Check for a draw**:
   - If no winner is detected and the board is full, set `status` to `"draw"`.
6. **Determine next state**:
   - If no winner is detected and the board is not full:
     - Set `status` to `"ongoing"`.
     - Set `next_player` to the other player (`"X"` if `current_player` was `"O"`, and vice versa).

---

## Edge Cases
1. **Out-of-range moves**: Moves with row or column indices outside the valid range (0–2).
   - Behavior is **unspecified**.
2. **Non-empty cell**: Attempting to place a mark on a cell that is already occupied.
   - Behavior is **unspecified**.

---

## Inferred Types or Structures (Non-Blocking)
1. **Board Representation**:
   - **Location**: Input `board`.
   - **Inference**: A 3x3 grid, represented as a nested structure (e.g., a list of lists or a 2D array).
   - **Basis**: Explicit reference to a "3x3 grid" in the draft.

2. **Move Representation**:
   - **Location**: Input `move`.
   - **Inference**: A structure containing two indices (row and column), each ranging from 0 to 2.
   - **Basis**: Explicit reference to "row and column index 0–2" in the draft.

3. **Status Values**:
   - **Location**: Output `status`.
   - **Inference**: An enumerated type with four possible values: `"ongoing"`, `"x_wins"`, `"o_wins"`, or `"draw"`.
   - **Basis**: Explicit list of valid values in the draft.

4. **Player Values**:
   - **Location**: Input `current_player` and output `next_player`.
   - **Inference**: An enumerated type with two possible values: `"X"` or `"O"`.
   - **Basis**: Explicit reference to `"X"` and `"O"` in the draft.

---

## Blocking Ambiguities
1. **Handling Invalid Moves**:
   - The draft does not specify how to handle moves on non-empty cells or out-of-range indices. This affects observable behavior (e.g., whether to return an error, ignore the move, or modify the game state unpredictably).

---

## Implementation Choices Left Open
1. **Board Representation**:
   - The exact data structure for the `board` (e.g., list of lists, 2D array, or custom type) is left to implementation.

2. **Move Representation**:
   - The exact structure for the `move` input (e.g., tuple, struct, or list) is left to implementation.

3. **Error Handling**:
   - The mechanism for handling invalid moves (e.g., returning an error, panicking, or silently ignoring) is left to implementation.

4. **Output Format**:
   - The exact format of the outputs (e.g., whether `updated_board` is returned as a new instance or modified in-place) is left to implementation.

5. **Win Detection Logic**:
   - The algorithm for detecting three-in-a-row (e.g., brute-force checks, precomputed patterns, or lookup tables) is left to implementation.