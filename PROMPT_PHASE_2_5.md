# Phase 2.5 implementation prompt

Paste the block below into a fresh `claude code` session at `/Users/rune/projects/reen`. Run `/clear` first so no prior context leaks in.

---

You are implementing Phase 2.5 of the Reen rewrite — the `App` artefact kind and a runnable scaffold. The authoritative specification is `PLAN.md` in this repository; Phase 2.5 sits conceptually with Phase 2 but is being executed after Phases 3–9 have shipped because the gap only became visible in real use. Read the Phase 2.5 section in full before planning. Phases 0–9 are merged.

## Why this phase exists (in one paragraph)

Snake has `drafts/app.md` that declares `Application Kind: cli_app` and describes its startup sequence, runtime, and main-loop behaviour. Nothing in Reen reads that file. The scaffolder walks data / projections / contexts / operations and emits `.rs` files under `src/`, but `Cargo.toml` has `[lib]` only — no `[[bin]]` — and there is no `src/main.rs`. `cargo run` fails on a fresh scaffold. This phase makes `drafts/app.md` schema-live enough to drive the binary entry point, leaves the rich prose sections as informational context for the implementer, and sets up Phase 10's `web_app` kind to land as an extension rather than a rewrite.

## Scope (do exactly these)

1. **New artefact kind.**
   - `ArtifactKind::App` in `src/draft_parser.rs`.
   - Filename convention: `drafts/app.md`. Exactly one per project — multiple is an ambiguity with a DCI-language message.
   - Per-kind parse path. Minimal schema:
     - `Application Kind:` (`cli_app` | `tui_app`). Any other value is an ambiguity naming the allowed kinds. `web_app` is reserved for Phase 10 — reject it here with a clear "Phase 10 owns web_app, not yet implemented" message rather than silently accepting.
     - `Entry Context:` a PascalCase name resolved against the manifest's `contexts[]`. Missing or unresolved → ambiguity.
     - `Entry Functionality:` a functionality name on that context. Optional when the entry context has exactly one public functionality (default to it); required and validated when there is more than one.
     - `Dependencies:` a list of crate names, one per line under the heading. Optional.
     - Every other section (Startup Sequence, Main Loop Behavior, Collaborators and Wiring, anything else the BA wrote) is preserved as prose on the prepared entry under a `notes` field but is **not** schema-enforced in this phase.

2. **Prepared schema.**
   - Add an `App` variant to the prepared entry enum in `src/prepared.rs` / `src/prepared_contracts.rs`.
   - Fields: `kind`, `entry_context`, `entry_functionality`, `dependencies[]`, `notes` (string, the remaining prose).
   - Do **not** bump the prepared-schema version for this. App is additive; existing `reen.prepare/v2` and `reen.prepare/v3` consumers continue to work. The App entry only appears when a project actually has an `app.md`.

3. **Manifest.**
   - The App entry does **not** enter the manifest's named-vocabulary sections. There is exactly one; it is addressed by kind, not by name.
   - Cross-draft references to "the app" are not expected; if a draft contains `@app/...`, fail resolution with a message suggesting the BA describe the flow in their own draft's prose instead.

4. **Codegen extension.**
   - In `src/codegen.rs`, after the existing per-artefact `.rs` emission, add an App pass that runs iff the project's prepared tree contains an `App` entry.
   - Emit `src/main.rs` with a minimal entry stub. Target shape (adjust imports for the actual generated module layout):
     ```rust
     use crate::contexts::<entry_context_snake>::<EntryContext>;

     fn main() -> anyhow::Result<()> {
         // Role players wired by the implementer.
         let ctx = <EntryContext>::new(todo!("wire role players"));
         ctx.<entry_functionality>()?;
         Ok(())
     }
     ```
     The function signature returns `anyhow::Result<()>` only if `anyhow` is in the declared dependencies; otherwise emit `fn main()` returning `()` with `.unwrap()` on the functionality call. Do not silently add `anyhow` — the BA has to declare it.
   - Patch `Cargo.toml`:
     - Parse with `toml_edit` (preserves comments and ordering).
     - Add a `[[bin]]` entry `name = "<package-name>"`, `path = "src/main.rs"` if one is not already present.
     - Add each dependency from the App entry's `dependencies[]` under `[dependencies]` with `"*"` as the default version spec unless already present. Never overwrite existing version pins. Never remove dependencies — even if the BA drops one from `dependencies[]`, codegen leaves it alone (removing is the user's call).
   - Idempotency: running `reen scaffold` twice produces byte-identical `Cargo.toml` and `src/main.rs` modulo a user's intervening edits. Test this explicitly.
   - **Never overwrite a `src/main.rs` that has been hand-edited.** Use the same convention as other scaffolded files: only overwrite if the file is absent, or if its top-level generated marker matches (insert a `// @generated by reen — edit freely; will be regenerated if this comment is removed` line at the top of the stub and have the scaffolder refuse to overwrite a file without it).

5. **Verify extension.**
   - If the prepared tree has an App entry, `reen verify` checks:
     - `src/main.rs` exists.
     - It contains a `fn main` that references the declared `EntryContext` by name.
     - It calls the declared entry functionality on that context.
     - `Cargo.toml` has a `[[bin]]` entry whose `name` matches the package name.
   - All drift is reported in DCI language ("the app declares `PlaySnake` as the entry context, but `src/main.rs` constructs `PauseMenu` instead"). No syn stack traces in the BA's face.

6. **Hooks.**
   - `.claude/hooks.json` (and the template twin under `templates/.claude/hooks.json` since Phase 8 has landed): PostToolUse Write on `drafts/app.md` triggers `reen prepare` on the affected draft. Verify wildcards already cover this if `drafts/**.md` is in place — add an explicit entry only if the existing wildcard does not match. PostToolUse Write on `src/main.rs` triggers `reen verify && cargo check`, same as other Rust files.

7. **Skill updates.**
   - Add a short App section to `templates/.claude/skills/reen-draft-format/SKILL.md` (and `reen-dci-authoring/SKILL.md` as appropriate): what `drafts/app.md` looks like, that exactly one exists per project, the three structured fields (Application Kind, Entry Context, Entry Functionality, Dependencies), and the convention that the rest is informational prose for Claude Code.
   - Run `reen init --upgrade` inside the repo so the root `.claude/` stays in sync. Phase 8 is landed; do not edit both locations.

8. **Migration of test projects.**
   - For each of `tests/snake`, `tests/money transfer`, `tests/Connor` (and `tests/world data` if Phase 7 has landed):
     - Update `drafts/app.md` to the new template: explicit `Application Kind`, `Entry Context`, `Entry Functionality`, `Dependencies` sections at the top; existing prose preserved below.
     - Run `reen prepare && reen scaffold` fresh.
     - Confirm the emitted `src/main.rs` matches (or is a clean superset of) the existing hand-written `main.rs`. If it is not a clean match, either the app.md is wrong or the scaffolder is — fix whichever is wrong in its own commit.
     - Confirm `Cargo.toml` now has the `[[bin]]` entry and that `cargo check && cargo build` succeeds and `cargo run` either runs or fails cleanly inside a `todo!()` (never on a missing entry).

9. **Tests.**
   - Parser unit tests for each App-field shape: happy path; `web_app` rejection; unresolved entry context; ambiguous entry functionality; missing required field.
   - Codegen tests:
     - Fresh scaffold of a project with a valid `app.md` emits `src/main.rs` and a `[[bin]]`-carrying `Cargo.toml`.
     - Second scaffold is byte-identical to the first.
     - A user-edited `src/main.rs` (with the `@generated` marker removed) is preserved across a scaffold run.
     - Declared dependencies are added; previously-added user dependencies are preserved.
   - Verify tests:
     - `src/main.rs` that constructs the wrong context → diff names the expected vs actual context.
     - Missing `[[bin]]` in Cargo.toml → targeted DCI-language message.
   - Regression: all existing golden-file tests for snake / money transfer / Connor continue to pass after the migration, and `cargo run` succeeds on each.

## Out of scope (strictly)

- `web_app` kind. Reserved for Phase 10.
- Multi-binary projects. One App per project. If a user needs multiple binaries, they can hand-maintain extra `[[bin]]` entries; scaffold will leave them alone.
- Generating an event loop, terminal setup, or UI rendering. The Main Loop Behavior prose stays as implementation guidance for Claude Code. Reen does not synthesise runtime plumbing in this phase.
- Schema-enforcing Startup Sequence / Main Loop / Collaborators sections. They ride as `notes` prose. If a later phase wants teeth, add them then.
- Changing the prepared-schema version. Additive only.
- Touching Phase 10 work.

## Commit pattern

- `phase 2.5: add App artefact kind to parser`
- `phase 2.5: prepared schema additions for App (no version bump)`
- `phase 2.5: codegen emits main.rs and Cargo.toml [[bin]]`
- `phase 2.5: verify checks App entry against main.rs and Cargo.toml`
- `phase 2.5: skill and hook updates for App`
- `phase 2.5: migrate snake to App kind`
- `phase 2.5: migrate money transfer to App kind`
- `phase 2.5: migrate Connor to App kind`
- `phase 2.5: tests and regression coverage`

If a defect in earlier phases surfaces while you work, fix it in its own `phase <n>: fix <defect>` commit before the dependent Phase 2.5 commit.

## Watchpoints

- **Cargo.toml round-trip.** Use `toml_edit`, not `toml`. `toml` will destroy comments and reorder sections. Every Cargo.toml edit must preserve user-authored content unchanged except for the specific additions.
- **`[[bin]]` idempotency.** Check for an existing `[[bin]]` entry whose `name` matches the package name **before** appending a new one. Don't key on `path` — users might have set `path = "src/bin/foo.rs"` and we should not duplicate.
- **Package name vs binary name.** The `[[bin]].name` should match the Cargo package name for ergonomic `cargo run` (no `--bin` flag needed). Derive from `[package].name`, not from the App draft. If the package name has hyphens, the binary name has hyphens; the entry context type name is PascalCase regardless.
- **Generated-marker discipline.** The comment at the top of `src/main.rs` is load-bearing for the overwrite-safety rule. Pick an exact string and assert on it in tests. Users who want to take ownership of `main.rs` delete the marker line; scaffold respects that.
- **Entry functionality defaulting.** Default to the only public functionality only when there is exactly one. If there are zero or more than one, require the draft to name it explicitly. Never guess.
- **Dependencies with `*`.** Using `"*"` as the default version spec is deliberate — it means "whatever the user pins is what they want." But warn (not fail) in `reen prepare` output when `*` would be written, so BAs see they should pin before publishing.
- **Hook wildcard matching.** `drafts/**.md` in the existing hook configuration covers `drafts/app.md` already on most glob engines, but some treat `**` as requiring at least one directory. Verify by trial, and if it does not match, add an explicit `drafts/app.md` entry.
- **Snake's existing main.rs.** Snake was hand-maintained with real terminal setup (crossterm raw mode, alternate screen, event loop). The scaffolder will emit a much thinner stub. During migration, preserve snake's existing main.rs — remove the generated marker or leave it untouched — and verify that it still matches the App draft's declared entry context and functionality. The point of the migration is schema-correctness, not to replace working hand-written code.
- **Error messages are BA-facing.** Every failure from this phase must be readable by someone who does not know Rust. No "expected `Ident`, found `Keyword`" style messages; instead "`drafts/app.md` declares `PlaySnakee` as the entry context but the manifest has no context by that name (did you mean `PlaySnake`?)".

## Finishing the phase

When you believe Phase 2.5 is done:

1. `cargo test` at the workspace root — green.
2. `cargo run -- init --upgrade` in the reen repo — `git status` clean (the dogfood check from Phase 8).
3. In each migrated test project, `reen prepare && reen scaffold && reen verify && cargo check` is clean, and `cargo run` either runs the app or fails inside a `todo!()` (never with a "no binary target" error from cargo).
4. A newly-created project with a minimal `app.md` scaffolds to a binary that links and starts.
5. `reen scaffold` on a project with a hand-edited `src/main.rs` (marker removed) leaves it untouched.
6. `reen scaffold` run twice produces byte-identical output.
7. A deliberately-broken `app.md` (unresolved entry context, ambiguous functionality, or `Application Kind: web_app`) fails with a targeted DCI-language message.
8. Update the task list: mark Phase 2.5 complete; unblock Phase 10 (which depends on it); leave Phase 7, 8, 9.5, 10, 11 pending as they stand.
9. Report back with a one-paragraph summary, the new App section of the prepared YAML for snake (as a representative example), and confirmation that `cargo run` now works in all migrated test projects.
