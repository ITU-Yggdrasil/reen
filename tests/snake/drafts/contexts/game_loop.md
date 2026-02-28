# GameLoopContext

## Description

GameLoopContext performs **one tick** of the Snake game by making the full collaboration explicit in one place:
user input and collision detection and score is handled. The context also handles game speec/difficulty by decreasing the tick duration

---

## Roles

- **snake**
  The snake itself, representing occupied cells and current direction. Provides steering and movement operations.

- **command**
  Reads keyboard input and yields the next direction intention.

- **food_dropper**
  RNG that places a new food coordinate when needed.

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
  - Find the next cell for the head based on the provided direction
    The new head cell is determined by the direction:
    - UP:    (head.x,     head.y + 1)
    - DOWN:  (head.x,     head.y - 1)
    - RIGHT: (head.x + 1, head.y)
    - LEFT:  (head.x - 1, head.y)
  

### command

- **next**
  Reads the latest input key.
  - if W/A/S/D pressed map to UP/LEFT/DOWN/RIGHT respectively and return that direction
  - otherwise return the current direction of the snake

### FoodDropper

- **drop**
  Produces a new food object with a coordinate within the board that is not occupied by the snake.
  (Implementation policy is up to you: retry sampling, sample from precomputed free cells, etc.)

---

## Functionality
- **new** takes a snake, a board, a random number generator and a game state and assigns them to the snake role, the food dropper, the board and the game state
- **current_board** returns a two dimensional array of characters.
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
     - get the desired direction from command.next
  
  3. obtain the new position without moving the snake yet (snake.next(new_direction))

  4. Test for collision at the new head position
    - The collisionType is Some(Obstacle) if the head location is also occupied by
      - any boundary cell (where x==0, y==0, x==board.width-1, y==board.height-1)
      - any snake segment coordinate excluding the head coordinate
    - the collisionType is Some(Food) if the coordiantes of the head equals the current food coordinate. 
    - otherwise the collisionType is None

  5. handle game logic:  
    - If collisionType is Some(Obstacle):
       - return None
    - snake = snake.set_direction(new_direction)
      - If collisionType is Some(food):
        - move the snake and mark that it should grow `snake.move(true)`
        - increment the score by 10
        - update state with new food placement from food_dropper.drop
        - return a new Game loop context based on the updated state (snake and game state)

      - otherwise:
        - move the snake without growing `snake.move(false)`
        - return a new Game loop context based on the updated state (snake and game state)
