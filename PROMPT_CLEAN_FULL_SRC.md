# `reen clean` should wipe all of `src/`

Paste the block below into a fresh `claude code` session at
`/Users/rune/projects/reen`. Run `/clear` first.

---

You are fixing a scope bug in `reen clean`. The current implementation
hardcodes three subdirectories (`src/contexts/`, `src/data/`,
`src/projections/`) as the set of reen-owned source. That list is wrong:
later phases added `src/apis/`, `src/components/`, `src/brand/`,
`src/routes/`, and `src/main.rs` (for App kinds), and future kinds will
add more.

The correct rule is simpler: **everything under `src/` is machine-owned**.
A reen project never contains hand-written Rust under `src/`. If a user
wants custom code, they either author a draft for it or put it outside
`src/`.

## Scope (do exactly these)

1. **Fix `run_clean_cmd` in `src/main.rs`.** Replace the hardcoded
   `["contexts", "data", "projections"]` list with a single check for
   `src/` as a whole. The summary output, confirmation prompt, and
   deletion should all treat `src/` as one unit. Continue to also remove
   `.reen/build.json`, `drafts/prepare/`, and
   `.reen/generated_files.json`.

2. **Update the help text / docstrings.** Anywhere the CLI help or
   doc-comments enumerate which directories `reen clean` touches, change
   to "all of `src/`".

3. **Update `templates/.claude/commands/clean.md`.** The slash command's
   description should reflect the new scope — it removes the tracker,
   prepared YAML, and the entire `src/` tree.

4. **Update `templates/CLAUDE.md`.** The "Build tracker" section (or
   wherever `reen clean` is described) should state that `src/` in its
   entirety is machine-owned. Remove any surviving language about
   specific subdirectories being owned — that was always the wrong
   framing.

5. **Update `templates/.claude/skills/reen-dci-code/SKILL.md`.** Same
   correction: `src/` as a whole is machine-owned; never hand-edit
   anything under it.

6. **Dogfood.** Run `reen init --upgrade` inside the repo and confirm
   `git status` only shows the intentional edits. Never edit repo-root
   `.claude/` or `CLAUDE.md` directly.

7. **Regression check.** In the test project where the bug was first
   observed (lupa or whichever), run `/clean` and confirm the full
   `src/` tree disappears. Then run `/prepare` and `/scaffold` and
   confirm the project rebuilds cleanly.

## Out of scope

- Any change to what gets generated. This is purely about cleanup scope.
- Migrating `reen clear` (the older subcommand). Leave its behaviour as
  it was.
- Touching reen's own `src/` protection — reen itself is not a
  reen-managed project; the rule applies to downstream projects only.
  The CLI runs against the current working directory's project, which
  is already correct.

## Commit pattern

- `tracker: reen clean wipes all of src/ (not just three subdirs)`

## Definition of done

- `reen clean --force` in a reen-managed project deletes the entire
  `src/` tree, the tracker, and the prepared YAML.
- `/clean` followed by `/prepare && /scaffold` rebuilds the project.
- No hardcoded list of source subdirectories survives in the codebase
  or in the templates.
