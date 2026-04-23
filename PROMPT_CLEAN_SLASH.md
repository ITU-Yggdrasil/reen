# `/clean` slash command prompt

Paste the block below into a fresh `claude code` session at
`/Users/rune/projects/reen`. Run `/clear` first.

---

You are adding a `/clean` slash command to Reen so the pipeline has full
parity with the other phases (`/prepare`, `/refine`, `/scaffold`,
`/implement`, `/audit`). The `reen clean` CLI subcommand already exists —
this work only adds the slash-command wrapper and the doc updates.

## Scope (do exactly these)

1. **Add the slash command.** Create
   `templates/.claude/commands/clean.md` matching the style of the
   existing commands in that directory. YAML frontmatter with `name: clean`
   and a one-line description. Body instructs the command to run
   `reen clean --force` and report what was removed (tracker file,
   prepared YAML, reen-owned Rust subtrees). Keep it terse — the other
   commands are 5–15 lines.

2. **Update `templates/CLAUDE.md`.** Under the "Slash commands" section,
   add a `/clean` entry describing what it does. Keep the ordering
   consistent with the pipeline flow.

3. **Dogfood.** Run `reen init --upgrade` inside the repo and confirm
   `git status` shows the new command file and the CLAUDE.md edit in the
   repo-root copies as well. Never edit repo-root `.claude/` or
   `CLAUDE.md` directly.

## Out of scope

- Any changes to `reen clean` CLI behaviour. That is already done.
- Adding `/clean` to the `plans/BUILD_TRACKER.md` history. The plan is
  already updated.

## Commit pattern

- `tracker: add /clean slash command for pipeline parity`

## Definition of done

- `/clean` is listed in `templates/CLAUDE.md` alongside the other slash
  commands.
- `templates/.claude/commands/clean.md` exists and invokes
  `reen clean --force`.
- `reen init --upgrade` leaves `git status` clean apart from the
  intentional new/edited files.
