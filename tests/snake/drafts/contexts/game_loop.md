# Context: GameLoop

## Purpose

GameLoop enacts the rules of the Snake game from one tick to the next. It decides movement, turning, collision outcomes, scoring, food replacement, and pacing.

## Role Players

- snake: Snake — the current snake body and travel direction
- command: CommandInput — the shared user input stream for capturing and reading gameplay actions
- food_dropper: a random number generator — chooses a free interior food position on the board, or no position if none is available
- game_state: GameState — holds the score, food placement, and round start time

## Props

- board: the base board picture for the current round; the outer ring is treated as walls and the interior is overlaid with snake and food each tick

## Role Methods

### snake
- read_body: provides the ordered body positions, head first
- read_direction: provides the current travel direction

### command
- capture_input: adds newly available key presses to the shared queue without waiting
- read_next_action: returns the next gameplay action from the shared queue, if one is available

### food_dropper
- choose_food_position: chooses a food placement on a free interior cell using the current board and snake, or returns no placement when none is available

### game_state
- read_score: returns the current score
- read_food: returns the current food placement, if any
- read_start_time: returns the recorded start time of the round
- update_score: returns the same game state with a different score
- update_food: returns the same game state with a different food placement

## Functionalities

### new
Creates a game loop context from a board, a snake, a shared command input, a food dropper, and a game state. The game state passed in is expected to have no food yet — the caller does not place initial food. The context asks the food dropper to choose an initial food position. If a position is returned, the game state is updated with that food placement. If no position is available (the board has no free interior cell), the game state is left without food. The stored game state always reflects the food dropper's choice, not the caller's.

### current_board
Produces the rendered board picture for the current tick. Starts from the base board prop, overlays each snake body position with the snake symbol, then overlays the food position (if any) with the food symbol. The returned board is the complete picture for this tick.

### get_score
Returns the current score from the game state.

### tick
Advances the game by one tick. The steps are:

1. Read the current UTC millisecond time.
2. Compute the elapsed whole seconds since the round started.
3. Compute the wait time in milliseconds: 100 divided by (1 plus the base-10 logarithm of (1 plus elapsed seconds)), with a minimum of 10 milliseconds.
4. Sleep for the computed wait time.
5. Ask the command role to capture pending input.
6. Read the next gameplay action from the command role. Only one action is consumed per tick even if multiple are available.
7. If a movement action was received and it neither repeats nor reverses the current direction, update the snake's direction.
8. Compute the next head position from the current head and the current direction. Up decreases y by 1, Down increases y by 1, Left decreases x by 1, Right increases x by 1.
9. If the next head lands on a wall cell, a snake body cell, or a position outside the board, the round ends and the player is dead.
10. If food exists and the next head matches the food position, grow the snake by one segment, increase the score by 10, and ask the food dropper for a replacement food position using the updated snake.
11. Otherwise, advance the snake without growing; leave score and food unchanged.
12. The player is alive.

Extensions:
- If no action is available, skip the direction update and continue from step 8.
- If a fire action is received, ignore it and continue from step 8 with the current direction.
- If the food dropper returns no replacement position, clear the food from the game state.

Input consumed in one tick is not replayed in the next.

## Scenarios

### wall collision ends the round
Given: the next predicted head position is a wall cell
When: tick is called
Then: the round ends and the player is dead

### food collision grows the snake
Given: the next predicted head position matches the food position
When: tick is called
Then: the snake grows by one segment, the score increases by 10, and new food is placed

### clear move advances the snake
Given: the next predicted head position is an empty interior cell
When: tick is called
Then: the snake advances without growing and the score is unchanged

### initial food is placed by the food dropper
Given: a board, a snake, and a game state with no food
When: new is called
Then: the stored game state has a food placement chosen by the food dropper
