---
name: clean
description: Remove tracker, prepared YAML, and all of src/ to reset the project to pre-prepare state
---

Run `reen clean --force` to remove all generated pipeline outputs and return the project to pre-prepare state.

Steps:
1. Run `reen clean --force`.
2. Report which items were removed (tracker file, prepared YAML, src/ tree).
3. Confirm that drafts are still intact.
