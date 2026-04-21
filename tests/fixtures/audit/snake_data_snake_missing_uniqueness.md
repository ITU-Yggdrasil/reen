### Finding 1

- **kind**: invariant_not_enforced
- **severity**: high
- **spec_citation**: Snake.md, Invariants / "All positions in the body are unique (no overlap)."
- **code_citation**: data/snake.rs:14
- **description**: The Snake::new constructor does not check that body positions are unique — the invariant "All positions in the body are unique" is not enforced, allowing overlapping snakes to be created
