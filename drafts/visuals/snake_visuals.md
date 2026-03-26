# Visual Style

## Description

This file defines the visual appearance of the Snake game in the terminal renderer.
It is purely visual and does not affect game logic, collision rules, or mechanics.

## Board Appearance

- **Board walls / boundary**: Red
- All wall cells (the outer border where x==0, y==0, x==width-1, or y==height-1) must be rendered in red.
- Use ANSI escape codes to display the walls in red color.
- The interior playing field remains the default background (usually black or dark).

## Snake Appearance
- Snake body ('s' characters) uses default terminal color (usually green or bright white).

## Food Appearance
- Food ('f' character) uses default terminal color (usually yellow or bright white).

## Score Line
- The "Score: XXX" line at the bottom uses default terminal color.

## Rendering Notes
- The TerminalRenderer must respect the colors defined in this visual style file.
- Walls must appear red while keeping the existing ASCII characters ('w' for walls, 's' for snake, 'f' for food, ' ' for empty space).
- This is a visual update only — no changes to Board logic, Snake movement, collisions, or GameState.

## Goal
Provide a quick visual test by making the game board (walls) red. 