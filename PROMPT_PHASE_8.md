# Phase 8 implementation prompt

Paste the block below into a fresh `claude code` session at `/Users/rune/projects/reen`. Run `/clear` first so no prior context leaks in.

---

You are implementing Phase 8 of the Reen rewrite — packaging and distribution via `reen init`. The authoritative specification is `PLAN.md` in this repository; Phase 8 sits immediately before "Testing strategy". Read that section in full before planning. Phases 0–7 are already merged.

## Scope (do exactly these)

1. **Create `templates/` as the bundle source of truth.**
   - Make a new top-level `templates/` directory.
   - Move the existing canonical assets into it: `.claude/`, `CLAUDE.md`, `DCI.md`, `DRAFT_FORMAT.md`. Keep them git-tracked.
   - Author `templates/reen.manifest.yml` as an empty-but-valid starter manifest (schema header + empty capability sections).
   - Author `templates/.gitignore.fragment` with the three lines reen wants in a project's gitignore: `drafts/prepare/`, `drafts/.apis/`, `target/`.
   - Author `templates/bundle.toml` declaring, per file, whether it is `managed` (reen upgrades it) or `seed` (reen drops once). `.claude/**`, `DCI.md`, `DRAFT_FORMAT.md` → managed. `CLAUDE.md`, `reen.manifest.yml`, every `.gitkeep` → seed.
   - Add `.gitkeep` files for each `drafts/` subdirectory: `drafts/`, `drafts/data/`, `drafts/projections/`, `drafts/contexts/`, `drafts/apis/`.

2. **Embed the bundle in the binary.**
   - Add `include_dir` (0.7 or current) to `Cargo.toml`.
   - Create `src/bundle.rs` exposing a typed view: iterate embedded files, read contents, read classification from `bundle.toml`, compute sha256 hashes. No filesystem I/O inside this module — it is the read-only in-memory view of what the binary ships.

3. **Implement `reen init`.**
   - New module `src/init.rs`.
   - New subcommand wired into `src/main.rs`: `reen init [--path <dir>] [--upgrade] [--check] [--force]`.
   - Fresh install path (no `.reen-bundle.yml` present):
     - Refuse if any managed file from the bundle already exists on disk. Tell the user to run `--upgrade` and exit non-zero.
     - Materialise every managed and seed file from the bundle into the target directory.
     - Merge `.gitignore.fragment` into the project's `.gitignore`: create it if absent; append any fragment line not already present if it exists. Preserve existing ordering.
     - Write `.reen-bundle.yml` with schema `reen.bundle/v1`, the binary version (`env!("CARGO_PKG_VERSION")`), a UTC timestamp, and a `managed_files[]` list of every managed file actually written with its sha256.
     - Print a summary grouped by classification: "Installed N managed files, M seed files, updated .gitignore."

4. **Implement `reen init --upgrade`.**
   - Read `.reen-bundle.yml`; refuse if absent (tell the user to run `reen init`).
   - For each managed file in the new bundle:
     - If present on disk and its current sha256 matches the stored install hash → overwrite from bundle.
     - If present on disk and its sha256 differs from the stored install hash → the user has edited it. Without `--force`, do not overwrite; collect into an "edited files blocking upgrade" list.
     - If absent on disk → write from bundle (a new managed file added since install).
   - For managed files present in the stored manifest but not in the new bundle: delete them (they were removed upstream).
   - Seed files are never touched.
   - After the writes, rewrite `.reen-bundle.yml` with the new version, timestamp, and hashes.
   - If any file was blocked, exit non-zero and print the list with a hint about `--force`.

5. **Implement `reen init --check`.**
   - Run the same diff engine as `--upgrade` but write nothing.
   - Exit code: 0 if up to date, 1 if changes are pending that would apply cleanly, 2 if some changes would be blocked by user edits.
   - Human-readable report of added / changed / removed / edited-blocking files.

6. **Dogfood the bundle.**
   - After steps 1–5 are green in isolation, run `cargo build && cargo run -- init --upgrade` inside the reen repo itself.
   - Confirm `git status` shows no tracked-file changes. If it does, the templates and the repo-root copies have drifted; reconcile by making the repo root match the templates.
   - Add an integration test `dogfood_is_clean` that clones (copies) the repo into a tempdir, runs `reen init --upgrade` against it, and asserts no file diffs.

7. **Migrate the test projects.**
   - In each of `tests/snake`, `tests/money transfer`, `tests/Connor` (and `tests/world data` if Phase 7 has landed):
     - Remove the committed `.claude/` tree, `CLAUDE.md`, `DCI.md`, `DRAFT_FORMAT.md` (whatever is present).
     - Run `reen init` in the project directory.
     - Confirm the project still builds end-to-end (`reen prepare && reen scaffold && reen verify && cargo check`).
   - One commit per project: `phase 8: migrate <project> to bundle`.

8. **Documentation.**
   - Update `README.md` with the install + init flow: `cargo install reen`, then `cd your-project && reen init`, then `reen init --upgrade` later.
   - One-paragraph "Upgrading" section naming `--check` and `--force`.
   - If `SETUP.md` exists (Phase 6), rewrite its manual-copy steps to `reen init`.

9. **Publishing prep (no publish).**
   - Audit `Cargo.toml` package metadata: `description`, `license`, `repository`, `readme`, `keywords`, `categories`. Fix anything missing or wrong.
   - Run `cargo publish --dry-run`; fix every warning it emits.
   - Do not run `cargo publish`. Publishing is the user's decision.

## Out of scope

- Actually publishing to crates.io in this phase.
- Injecting reen as a Rust dependency in target projects. Reen is a CLI tool consumed from `PATH`.
- Generating starter drafts (e.g. a sample data draft). The directory structure is scaffolded; authoring is the BA's job.
- A separate template repo. The bundle lives with the binary; that is the whole point of this phase.
- Touching v0-archive or any prior commits.
- Any new pipeline behaviour. Phase 8 is packaging only — it installs what Phases 3–7 produced.

## Commit pattern

- `phase 8: add templates directory and bundle index`
- `phase 8: embed bundle and implement reen init`
- `phase 8: implement reen init --upgrade and --check`
- `phase 8: dogfood the bundle in the reen repo`
- `phase 8: migrate <project> to bundle` (one commit per test project)
- `phase 8: documentation and publishing prep`

If a defect in Phase 1–7 surfaces while you work, fix it in its own `phase <n>: fix <defect>` commit before the dependent Phase 8 commit.

## Watchpoints

- **Determinism and stable file order.** The bundle index must produce the same serialised `.reen-bundle.yml` contents on repeated runs for the same bundle. Sort `managed_files[]` by path. Use a fixed timestamp format (RFC 3339, UTC, seconds precision) — not a float — so dogfood-idempotency holds.
- **Classification source of truth.** `bundle.toml` is the single source of truth for managed vs seed. Every file in `templates/` must be classified; the loader rejects unclassified files with a clear error.
- **`.gitignore` merge must be idempotent.** Running `reen init` followed by `reen init --upgrade` must not duplicate gitignore lines. The merge compares existing lines against the fragment and only appends missing ones.
- **Hash scheme for managed files.** Use sha256 of the raw bytes (no newline normalisation) and store as lowercase hex. Document this so a future change to the scheme is an explicit bundle version bump, not an accidental breaking change.
- **User edits to managed files on upgrade.** The `--force` flag must print what it is overwriting. Silent overwrites are worse than a refusal.
- **Dogfood symmetry.** `templates/` is the source of truth; the repo-root copies are regenerated. If a user opens a PR that edits `.claude/commands/implement.md` at the repo root, `reen init --upgrade` (or the CI check) will refuse to apply it cleanly because the template wasn't updated. Add a short note in `CLAUDE.md` telling contributors to edit `templates/` and then run `reen init --upgrade`.
- **Cross-platform paths.** Use `camino` or `std::path` consistently; do not concatenate strings with `/`. The bundle is embedded with forward slashes (as `include_dir` provides) — normalise at the boundary.
- **Managed-file delete on upgrade.** Carefully: only delete files listed in the stored manifest's `managed_files[]` that are absent from the new bundle. Never delete seed files. Never delete files the user created that aren't in the manifest.

## Finishing the phase

When you believe Phase 8 is done:

1. `cargo test` at workspace root — green.
2. `cargo run -- init --upgrade` in the reen repo — `git status` clean.
3. `cargo run -- init --check` in the reen repo — exit 0.
4. `cargo publish --dry-run` — zero warnings.
5. In a tempdir, `cargo run -- init --path <tempdir>` produces a working project layout; running `reen prepare` in it exits cleanly (possibly with "no drafts to prepare").
6. Update the task list: mark Phase 8 complete; note any follow-ups (e.g. publishing to crates.io, Claude Code plugin distribution).
7. Report back with a one-paragraph summary, the new CLI surface, and a recommendation on whether to publish and when.
