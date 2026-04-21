---
name: refine
description: Review drafts for completeness, concreteness, and ambiguity, then resolve issues
---

Run `reen refine` to review raw drafts for DCI completeness, concreteness, and ambiguity.

Steps:
1. Run `reen refine --verbose` in the project root.
2. If findings are reported, present each one with its category and location.
3. For each finding, propose a concrete fix to the draft file.
4. After the user approves edits, apply them and re-run `reen refine` to confirm resolution.
5. When refine passes, suggest running `/prepare` to generate updated artifacts.
