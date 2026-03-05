# GameLoopContext

## Description

GameLoopContext performs **one tick** of the Snake game by making the full collaboration explicit in one place:
user input and collision detection and score is handled. The context also handles game speec/difficulty by decreasing the tick duration

---

## Roles

- **snake**
  The snake itself, representing occupied cells and current direction. Provides steering and movement operations.

- **command**
  The command role is played by keyboard input from standard input (`stdin`), not by a separate domain object.
  It yields the next direction intention for the snake.

- **food_dropper**
  RNG that places a new food object when needed.

- **game_state** The state of the game represented by a GameState object

---

## Props

- **board**
  Board dimensions and boundary definition (used to derive boundary obstacle cells).

---

## Role methods

### snake

- **head**
  Returns the position of the snake head.

- **set_direction(new_direction)**
  If the new direction is opposite to the current direction or the same as the current direction then the existing snake object is returned.
  Otherwise a new snake object is returned with the direction altered.

- **next** 
  - Find the next cell for the head based on the current direction of the snake
    The new head cell is determined by the direction:
    - UP:    (head.x,     head.y + 1)
    - DOWN:  (head.x,     head.y - 1)
    - RIGHT: (head.x + 1, head.y)
    - LEFT:  (head.x - 1, head.y)

- **move** accepts a flag for whether the snake should grow or not. 
  - call snake.next to get the new head position
  - push this position to the front of snake.body
  - if snake shouldn't grow pop the last element of body
  - create and return a new snake with the new body and the same direction

### command

- **next**
  The command role is a FIFO buffer of keystrokes read from `stdin`.
  - In each call to `next`, pop keystrokes from the front of the buffer until:
    - a movement key is found (`W`/`A`/`S`/`D`, case-insensitive), in which case return `UP`/`LEFT`/`DOWN`/`RIGHT`, or
    - the buffer becomes empty.
  - If the buffer becomes empty before any `W`/`A`/`S`/`D` is found, return None

### food_dropper

- **drop**
  Produces a new food object with a coordinate within the board that is not occupied by the snake nor by the boundary itself.
  (Implementation policy is up to you: retry sampling, sample from precomputed free cells, etc.)

---

## Functionality
- **new(board, snake, food_dropper, game_state) -> GameLoopContext** constructs a new context by assigning the provided collaborators to their roles/props. The `command` role is implicitly bound to keyboard input (`stdin`) by the runtime and is therefore not passed as a constructor argument.
  - `new` uses the shared command keystroke buffer bound to `stdin`.

- **current_board** returns a two dimensional array of characters, indices into the array matches coordinates on the board. i.e. array[0][1] matches the coordinates (x=0,y=1) .
  - 'w' for wall at the boundary
  - ' ' for unoccupied cells
  - 's' for cells occupied by the snake 
  - 'f' for where the food is placed

- **get_score** returns the current score of the game

- **tick**
  Executes one tick and returns:
  - `Some(new GameLoopContext)` if the game continues
  - `None` if the game ends

  Script:

  1. **calculate game speed**. We should start at 10 ticks pr second and then increase that in a logarithmic fashion (the exact algorithm is a free implementation choice). sleep for the calculated delay before proceeding
     
  2. **Steering**
      - next_direction = command.next
      - if next_direction is None then snake = snake
      - else snake = snake.set_direction(next_direction)
  
  3. obtain the new position without moving the snake yet (snake.next())

  4. Test for collision at the new head position
    - The collisionType is Some(Obstacle) if the head location is also occupied by
      - any boundary cell (where x==0, y==0, x==board.width-1, y==board.height-1)
      - any snake segment coordinate excluding the head coordinate
    - the collisionType is Some(Food) if Food is not None and the coordiantes of the head equals the current food coordinate. 
    - otherwise the collisionType is None

  5. handle game logic:  
    - If collisionType is Some(Obstacle):
       - return None

    - If collisionType is Some(food):
      - move the snake and mark that it should grow `snake.move(true)`
      - increment the score by 10
      - update state with new food placement from food_dropper.drop
      - return a new Game loop context based on the updated state (snake and game state)

    - otherwise:
      - move the snake without growing `snake.move(false)`
      - return a new Game loop context based on the updated state (snake and game state)
