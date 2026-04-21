# Data: UserAction

## Purpose

UserAction describes the gameplay actions that can arrive from user input.

## Attributes

The variants are:

- Movement: a movement command carrying one Direction
- Fire: a separate action triggered by the space key

## Invariants

## Operations

## Scenarios

### movement action
Given: the user presses the "w" key
When: the key is mapped to an action
Then: the result is a Movement action carrying the Up direction

### fire action
Given: the user presses the space key
When: the key is mapped to an action
Then: the result is a Fire action
