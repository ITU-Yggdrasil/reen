# tick_tack_toe

## Description
A two-player game played on a **3x3 grid**. Players take turns placing their mark (`"X"` or `"O"`) on empty cells. The game concludes when one player achieves **three in a row** (horizontally, vertically, or diagonally) or all cells are filled without a winner.

---

## Responsibilities
1. **Track the current board state**: Maintain a 3x3 grid representing the game board.
2. **Track the current player**: Identify which player (`"X"` or `"O"`) is expected to make the next move.
3. **Apply a player's move**: Update the board state if the chosen cell is empty.
4. **Detect game status**: Determine if the game is:
   - **Ongoing**: No winner and empty cells remain.
   - **Won**: A player has achieved three in a row.
   - **Draw**: All cells are filled without a winner.

---

## Inputs
| Input            | Description                                                                                     |
|------------------|-------------------------------------------------------------------------------------------------|
| `current_player` | The player making the move. Must be either `"X"` or `"O"`.                                     |
| `board`          | A 3x3 grid representing the current state of the game. Each cell is `"X"`, `"O"`, or empty.    |
| `move`           | The position chosen by the current player, specified as `(row, column)` indices (0–2).         |

---

## Outputs
| Output          | Description                                                                                     |
|-----------------|-------------------------------------------------------------------------------------------------|
| `updated_board` | The board state after applying the move.                                                        |
| `next_player`   | The player expected to make the next move (`"X"` or `"O"`). Only valid if `status` is `"ongoing"`. |
| `status`        | The result of the move. Possible values: `"ongoing"`, `"x_wins"`, `"o_wins"`, or `"draw"`.      |

---

## Main Flow
1. **Receive inputs**: The system is provided with `current_player`, `board`, and `move`.
2. **Validate move**:
   - If the chosen cell is **not empty**, the move is treated as **invalid**.
   - If the move is invalid, the behavior is **unspecified**.
3. **Apply move**: Place the `current_player`'s mark (`"X"` or `"O"`) in the chosen cell.
4. **Check for a winner**:
   - If the `current_player` has achieved **three in a row** (horizontally, vertically, or diagonally), set `status` to `"<player>_wins"` (e.g., `"x_wins"` or `"o_wins"`).
5. **Check for a draw**:
   - If there is no winner and the board is **full**, set `status` to `"draw"`.
6. **Determine next state**:
   - If there is no winner and the board is **not full**, set `status` to `"ongoing"` and switch `next_player` to the other player (`"X"` or `"O"`).

---

## Edge Cases
1. **Out-of-bounds move**: The `move` specifies a row or column index outside the valid range (0–2).
   - Behavior is **unspecified**.
2. **Non-empty cell**: The chosen cell is already occupied by `"X"` or `"O"`.
   - Treated as an **invalid move** (behavior is **unspecified**).

---

## Inferred Types or Structures (Non-Blocking)
| Inferred Item       | Location in Specification       | Inference Made                          | Basis for Inference                     |
|---------------------|---------------------------------|-----------------------------------------|-----------------------------------------|
| `board` structure   | Inputs, Main Flow               | 3x3 grid of cells                       | Described as a "3x3 grid" in the draft. |
| `move` structure    | Inputs                          | Tuple or struct with `row` and `column` | Described as `(row, column)` indices.   |
| `status` values     | Outputs                         | Enumerated set of string values         | Explicitly listed as `"ongoing"`, `"x_wins"`, `"o_wins"`, or `"draw"`. |

---

## Blocking Ambiguities
1. **Handling of invalid moves**:
   - The draft states that moves on non-empty cells or out-of-bounds indices are treated as "invalid," but does not specify the expected behavior (e.g., error, ignore, or reject).
   - This affects externally observable behavior and must be clarified.

---

## Implementation Choices Left Open
1. **Representation of the `board`**:
   - The draft does not specify the exact data structure for the 3x3 grid (e.g., array, vector, or nested lists).
   - This is a non-blocking technical choice.
2. **Representation of the `move`**:
   - The draft describes the `move` as `(row, column)` indices but does not specify whether it should be a tuple, struct, or another data structure.
   - This is a non-blocking technical choice.
3. **Output format for `updated_board`**:
   - The draft does not specify whether the `updated_board` should be returned as a new instance or modified in-place.
   - This is a non-blocking technical choice.
4. **Case sensitivity for `current_player` and `next_player`**:
   - The draft does not specify whether `"X"` and `"O"` are case-sensitive.
   - This is a non-blocking technical choice.