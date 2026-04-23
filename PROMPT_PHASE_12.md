# Phase 12 implementation prompt

Paste the block below into a fresh `claude code` session at `/Users/rune/projects/reen`. Run `/clear` first so no prior context leaks in.

---

You are implementing Phase 12 of the Reen rewrite — compile-time feature flags on artefacts and multiple apps per project. The authoritative specification is `PLAN.md` in this repository; Phase 12 follows Phases 10 and 11 and extends Phase 2.5's `App` artefact kind from a singleton into a plural. Read the Phase 12 section in full before planning. Phases 0–11 are merged.

## Why this phase exists (in one paragraph)

Snake has a renderer choice that today lives in an environment variable (`SNAKE_RENDERER=string`), with both the terminal renderer and the string renderer shipped in every build. There is no way at the draft level for a BA to say "TerminalRenderer is terminal-only" or "this screen exists only in the web variant," and no way to produce more than one binary from a single project. Cargo's native mechanism for this — `[features]` in `Cargo.toml`, `#[cfg(feature = "…")]` gates in source, `[[bin]]` entries with `required-features` — is a capability Reen currently ignores. Phase 12 reintroduces it, starting at the draft level: any artefact can declare a `Feature:`, any App can declare `Requires Features:`, and the scaffolder emits gated modules, a managed `[features]` block, and one `[[bin]]` per declared app. The test case is a snake split: `snake_terminal` and `snake_web` share a core game lib and compile to two binaries with disjoint renderer trees.

## What Phase 12 is and is not

*Is:* add an optional `Feature:` line to context / projection / data / component / operation drafts; let projects declare multiple apps via `drafts/apps/<app_name>.md`; let each app declare a `Requires Features:` closure; emit `#[cfg(feature = "…")]` gates at the `mod` level in generated code; manage the `[features]` section of `Cargo.toml` with `toml_edit`; emit one `[[bin]]` per app with `required-features`; add verify cases that walk the transitive closure per app and flag unsatisfiable configurations; migrate snake to two apps and build out the web variant as the primary test case.

*Is not:* automatic feature inference (every feature is explicitly named on an artefact); runtime feature toggles (env-var / config-file based) — those remain the implementer's business; per-target-arch `#[cfg(target_arch = "…")]` gating beyond what Phase 10 already does for WASM; a new artefact kind for "feature bundles" — apps serve that role; removing features from `Cargo.toml` when a draft drops its `Feature:` line (additive only, same rule as Phase 2.5 dependencies); changing Cargo's feature-unification semantics; publishing crates.

## Scope (do exactly these)

1. **`Feature:` metadata on artefact drafts.**
   - In `src/draft_parser.rs`, extend the per-kind parse path for `Context`, `Projection`, `Data`, `Component`, and `Operation` to accept an optional `Feature:` line in the draft's metadata block (wherever `Dependencies:` / `Source:` / similar already live — a uniform location is best, pick the one most drafts already use for metadata).
   - Exactly one `Feature:` line per artefact, carrying a single snake-case name. Multiple features on one artefact → ambiguity (`this artefact is tagged with two features, 'x' and 'y'; pick one or introduce an app-level feature that unions them`).
   - Unfeatured artefacts remain the default. Do not require the line.
   - Feature names must match `[a-z][a-z0-9_]*`. Reject kebab-case at parse time with a DCI-language message — Cargo allows both but Reen picks snake_case for consistency with the rest of the prepared vocabulary.

2. **Multi-app directory form.**
   - Accept `drafts/apps/<app_name>.md` as the new canonical location for App drafts. The singleton `drafts/app.md` continues to parse but `reen prepare` emits a one-time migration notice suggesting the move (not an error — migration is user-paced).
   - `<app_name>` derived from the filename (snake_case, no extension) is the app's identifier.
   - Each app carries the Phase-2.5 structured fields (`Application Kind`, `Entry Context`, `Entry Functionality`, `Dependencies`) plus an optional new `Requires Features:` list — one feature per line under the heading, same shape as `Dependencies:`.
   - Exactly one `Application Kind: web_app` app per project (keep Phase 10's invariant: one web app per workspace; the other apps must be `cli_app` or `tui_app`).
   - Zero apps in a project is still legal (pure library projects exist); the verify / scaffold paths for App simply don't fire.

3. **Prepared schema.**
   - Add `feature: Option<String>` to the prepared entry for every artefact that accepts the new draft line.
   - Replace the singleton prepared App entry with a `Vec<App>` (or an `apps:` field at the prepared-tree root). Each App entry gains a `name: String` and `requires_features: Vec<String>`.
   - Version bump: this is a structural change, so bump the prepared schema to `reen.prepare/v4`. The `v3` reader must still be able to read pre-Phase-12 files (migrate transparently: a lone `app:` field becomes `apps: [<that entry with name="app">]`). The `v4` writer emits the new shape on every `reen prepare` run.
   - Update every existing golden-file test under `tests/fixtures/` to the `v4` shape. This will be a large diff; commit it separately from the codegen work so the snapshot churn is reviewable.

4. **Codegen.**

   *Feature gates.* When a prepared artefact carries a `feature`, the scaffolder wraps the corresponding `mod <snake>;` declaration in `crates/app-*/src/<kind>/mod.rs` (or `src/<kind>/mod.rs` for single-crate projects) with:
   ```rust
   #[cfg(feature = "<name>")]
   pub mod <snake>;
   ```
   The module file itself is emitted unconditionally — `#[cfg]` lives only on the `mod` line. This keeps on-disk output deterministic and lets `rust-analyzer` see every module when the user has `"rust-analyzer.cargo.allFeatures": true`.

   *Cargo.toml `[features]` block.* Scaffolder-owned, managed via `toml_edit`. After each scaffold run, the `[features]` section contains:
   - One entry per feature named on any artefact: `<feature> = []` (empty feature vector — it names itself, nothing more).
   - One entry per declared app: `<app_snake> = ["<f1>", "<f2>", ...]` — the app's `requires_features` list verbatim.
   - A `default = [...]` entry is **not** scaffolder-managed. If present, Reen leaves it alone; if absent, Reen does not create it. Defaults are a user-facing decision.
   - Stale entries (features in `[features]` that no artefact and no app names anymore) are preserved with a `# no longer declared; delete if unused` comment appended by `toml_edit`. Do not delete them — consistent with Phase 2.5's "never remove dependencies" rule.
   - The `[features]` section is emitted **after** `[dependencies]` and **before** any `[[bin]]` entries, for readability. Assert on ordering in a test.

   *`[[bin]]` entries.* One per declared app, emitted after `[features]`:
   ```toml
   [[bin]]
   name = "<app_snake>"
   path = "src/bin/<app_snake>.rs"
   required-features = ["<feature-list>"]
   ```
   `required-features` is the app's `requires_features` list. If the list is empty, omit the `required-features` key entirely (Cargo treats no-key as no-requirement).

   *Per-app `src/bin/<app_snake>.rs`.* Same emission pattern as Phase 2.5's `src/main.rs`, with the `@generated` marker convention. For `cli_app` / `tui_app` apps, the template is identical to Phase 2.5's. For the `web_app` app, the binary lives in `crates/app-server/src/bin/<app_snake>.rs` (Phase 10's workspace layout) and boots the server.

   *Retirement of the singleton `src/main.rs` path.* Once a project has `drafts/apps/`, Reen emits into `src/bin/` and does not touch `src/main.rs`. If both exist (legacy `drafts/app.md` + new `drafts/apps/*.md`), prepare emits an ambiguity ("this project has both a singleton app draft and an apps directory — pick one"). The migration for snake / money transfer / Connor moves everything to `drafts/apps/`; no project retains the singleton after Phase 12 lands.

5. **Verify extensions.**
   - **Feature declaration alignment.** Every feature named on a scaffolded `#[cfg(feature = "<x>")]` line or an `[[bin]].required-features` entry must be declared in `[features]`. Every feature in `[features]` whose value is `[]` must be named by at least one artefact (or else it's dead; flag as drift with the "delete if unused" comment).
   - **App closure satisfiability.** For each declared app, walk the transitive context graph starting from the entry functionality (or from every routed functionality for `web_app`). For every reachable context that carries a `feature`, that feature must appear in the app's `requires_features` closure. Report violations in DCI language: "`snake_terminal` does not require feature `web_renderer`, but its entry functionality `PlaySnake/run` transitively reaches the context `WebRenderer` which is gated by `web_renderer`."
   - **Feature-name hygiene.** Enforce the `[a-z][a-z0-9_]*` pattern at verify time as well as parse time (defence in depth; a hand-edit to prepared YAML shouldn't slip past).
   - **App-name uniqueness.** Two apps with the same snake-case name → verify drift ("two apps both named `snake_terminal`; app names are the `[[bin]].name` and must be unique").
   - **`web_app` cardinality.** At most one `web_app` per project — the Phase 10 invariant now lives in verify rather than in parse, because the parse-time check (per-file) can't see the project-wide count.

6. **CLI.**
   - No new subcommands. `reen prepare`, `reen scaffold`, `reen verify` all absorb the new shape silently.
   - `reen scaffold --app <name>` is tempting (scaffold only the artefacts reachable from one app) and should be considered, but **out of scope** for this phase. Document as a candidate for a later phase if developer ergonomics demand it.

7. **Hooks.**
   - No new hooks. The existing `drafts/**.md` pattern covers `drafts/apps/*.md`; the existing `src/**.rs` and `crates/**/src/**.rs` patterns cover the new `src/bin/*.rs` locations.
   - Verify the wildcards by trial. If any existing hook explicitly matched `drafts/app.md` as a literal path, broaden it to `drafts/app.md` OR `drafts/apps/*.md`.

8. **Skill updates.**
   - `templates/.claude/skills/reen-draft-format/SKILL.md`: add the `Feature:` line to the per-kind draft templates for context / projection / data / component / operation, with a short example; add a section on multi-app projects covering `drafts/apps/<name>.md` and the `Requires Features:` heading.
   - `templates/.claude/skills/reen-dci-authoring/SKILL.md`: add a short note on when a feature is appropriate ("when two artefacts would not coexist in the same running binary — e.g. two renderers for the same data") and when it is a code smell ("when a feature gate is being used to paper over an unclear invariant that belongs in the data type").
   - `templates/.claude/skills/reen-dci-code/SKILL.md`: cover the emission shape — `#[cfg(feature = "…")]` on `mod` lines, `required-features` on `[[bin]]`, the dead-feature comment convention.
   - Run `reen init --upgrade` to sync the repo root.

9. **Migration of test projects.**

   *Money transfer and Connor.* Each becomes a one-app project under the new directory form. Move `drafts/app.md` → `drafts/apps/<project_name>.md`. No features declared. After re-scaffold, `cargo run --bin <project_name>` works identically to before. The only visible change is the prepared YAML shape (v3 → v4) and the source path (`src/main.rs` → `src/bin/<project_name>.rs`). Regression tests: a golden-file comparison of the produced binary's behaviour — same input, same output, byte-identical where possible.

   *Snake — the load-bearing test case.* This is the core deliverable of Phase 12:
   - Split the existing `TerminalRenderer` context draft: keep `TerminalRenderer` (`Feature: terminal_renderer`), unchanged in content, now gated.
   - Introduce a new `WebRenderer` component (Phase 10 component kind, `Feature: web_renderer`) that renders the board and score as HTML. Its props mirror `StringRenderer`'s inputs. One `render` method; bodies `todo!()` for the implementer to fill.
   - `StringRenderer` (projection), `Board` (data), `Snake` (data), `Position` (data), `Direction` (data), `GameState` (data), `PlayerState` (data), `CommandInput` (context), `GameLoop` (context) — all remain unfeatured, shared by both apps.
   - For the web variant: author a minimal Phase 10 web app — a single screen component that embeds `WebRenderer` and a text input for commands, a one-route routing table (`/` → that screen), a brand (use `snake` as the brand name, colour placeholders to be filled), and a `snake_web` app draft with `Application Kind: web_app` and `Requires Features: web_renderer`.
   - For the terminal variant: a `snake_terminal` app draft with `Application Kind: tui_app`, `Entry Context: GameLoop`, `Entry Functionality: run` (or whatever the current entry is named), `Requires Features: terminal_renderer`.
   - `drafts/app.md` is deleted; both apps live under `drafts/apps/`.
   - `cargo build --no-default-features --features snake_terminal` produces a binary that runs in the terminal and contains no web-crate dependencies linked in (check via `cargo tree -e features` and `nm`).
   - `cargo build --no-default-features --features snake_web` produces a binary that boots the Phase 10 server and contains no crossterm symbols.
   - The existing snake integration test still passes under `snake_terminal`. A new integration test for `snake_web` renders a fixture board and asserts the HTML contains the expected cells.

   *Lupa — secondary validation.* If Phase 11's Lupa has shipped by the time Phase 12 is underway, add a compile-time feature `sandbox_mode` (gating the live-Nordea adapter) and confirm it composes cleanly (both apps still build, both test suites still pass). If Lupa has not shipped, skip this — it becomes a Phase 11 follow-up.

10. **Tests.**
    - **Parser.** `Feature:` happy path on each supported kind; multi-feature rejection; kebab-case rejection; unknown-header position rejection (feature must live in the metadata block, not mid-prose).
    - **Parser (apps dir).** Single-file `drafts/apps/foo.md` happy path; multiple apps; duplicate names (same snake-case from two different file names that collide after normalization); `Requires Features:` empty list and non-empty list; legacy `drafts/app.md` coexisting with `drafts/apps/` (ambiguity); no-apps project (pure lib) scaffolds cleanly without `[[bin]]`.
    - **Prepared v4.** Snapshot tests for the new shape across the snake / money transfer / Connor migrations. A v3-to-v4 upgrade test: a hand-written v3 prepared file is read by the v4 reader and produces the expected v4 shape.
    - **Codegen.**
      - Feature gate on `mod` lines (not on the file); assert via text inspection of the emitted `mod.rs`.
      - `[features]` block ordering: features before `[[bin]]`, alphabetical within the section, dead features preserved with the comment.
      - Multiple `[[bin]]` entries with correct `required-features`; absent `required-features` when the list is empty.
      - `src/bin/<app>.rs` emission and `@generated` marker; hand-edit preservation (marker removed → scaffold leaves the file alone).
      - Idempotency: scaffold twice, byte-identical output across both Cargo.toml and every generated source file.
    - **Verify.**
      - Clean scaffold with two apps and three features passes.
      - Drift: app does not require a feature that a reachable context requires → DCI-language message names the app, the context, and the missing feature.
      - Drift: feature declared in `[features]` but named by no artefact → informational, not a hard error (drift tier: `low`).
      - Drift: two apps with the same name → hard error.
      - Drift: two `web_app` apps → hard error.
    - **Round-trip (ignored by default, run in CI with `--test-feature-flags`).**
      - snake: `cargo build --no-default-features --features snake_terminal` succeeds; produced binary does not depend on the web-renderer crate (parse `cargo tree` output).
      - snake: `cargo build --no-default-features --features snake_web` succeeds; binary does not depend on crossterm.
      - snake: `cargo build --all-features` succeeds (proves features compose).
    - **Regression.** Money transfer and Connor scaffolds produce the same runtime behaviour as pre-Phase-12. Snake's terminal binary, when built under `snake_terminal`, passes the existing snake integration test.

## Out of scope (strictly)

- Automatic feature inference from artefact location or name. A context named `WebRenderer` is not implicitly gated on a `web_renderer` feature — the author writes the line.
- Feature unification algorithms beyond what Cargo does natively. Reen declares features; Cargo decides what is on in any given build.
- Runtime feature flagging (feature flags as service toggles, A/B tests, gradual rollouts). That is a different problem domain and lives in the implementer's body code.
- Removing dead entries from `Cargo.toml`'s `[features]` block. Additive only.
- Feature dependencies between features (`web = ["renderer", "router"]` style meta-features). The app artefact already serves as the composing unit; adding a second tier of composition is scope creep.
- Cross-project feature vocabularies. Features are per-project.
- A `FeatureBundle` artefact kind. Apps already name their closures.
- Per-target-arch gates like `#[cfg(target_arch = "wasm32")]` — Phase 10 owns that axis, and the two axes compose through Cargo naturally.
- Changing Phase 10's four-crate workspace layout. Feature-gated modules live within whichever crate the artefact already belongs to; features do not cross crate boundaries structurally.

## Commit pattern

- `phase 12: Feature: metadata on context/projection/data drafts`
- `phase 12: Feature: metadata on component/operation drafts`
- `phase 12: drafts/apps/ directory form for App drafts`
- `phase 12: prepared schema v4 (apps[], feature on artefacts)`
- `phase 12: codegen emits #[cfg(feature = "…")] on mod lines`
- `phase 12: codegen manages [features] in Cargo.toml`
- `phase 12: codegen emits [[bin]] per app with required-features`
- `phase 12: verify — feature declaration alignment and app closure satisfiability`
- `phase 12: skill updates for features and multi-app`
- `phase 12: migrate money transfer to apps/ directory form`
- `phase 12: migrate Connor to apps/ directory form`
- `phase 12: migrate snake — split into snake_terminal and snake_web`
- `phase 12: snake web variant — WebRenderer component and Phase 10 shell`
- `phase 12: tests — parser, codegen, verify, round-trip`

If a defect in earlier phases surfaces while you work, fix it in its own `phase <n>: fix <defect>` commit before the dependent Phase 12 commit.

## Watchpoints

- **Prepared-schema version bump.** This is the first real bump since `v3`. Exercise both directions: v3 read → v4 internal representation → v4 write round-trips; v4 read passes through unchanged. The golden-file churn is large; commit the snapshot updates separately from the code that changes them.
- **`toml_edit` and `[features]`.** Cargo accepts either `feature_name = []` or `"feature-name" = []`. Reen uses snake_case, so no quoting is required, but the parser must handle both forms in existing files gracefully. When appending a new feature, emit the unquoted form.
- **Feature ordering inside `[features]`.** Alphabetical within the section is the rule, with `default = [...]` (if present) staying wherever the user put it. When inserting a new feature, bisect the existing alphabetical list and place it in order. Scaffold idempotency depends on this — two runs on the same project must produce byte-identical output.
- **Dead-feature comment.** `# no longer declared; delete if unused` is the exact canonical string. Assert on it in a test. If users delete the comment and keep the feature, Reen should not re-add the comment on the next scaffold (the user has acknowledged it).
- **App `required-features` vs. feature transitivity.** Cargo features are additive: `cargo build --features snake_terminal` enables `snake_terminal`'s `required-features` list. In `[features]`, `snake_terminal = ["terminal_renderer"]` means "turning on snake_terminal also turns on terminal_renderer." This is exactly the composition Reen wants — verify it actually works end-to-end before declaring victory. A unit test that runs `cargo metadata --features snake_terminal` on a fixture and asserts the resolved feature set is the cheapest check.
- **`[[bin]].required-features` vs `[features]`.** Both can restrict what is compiled into a given binary. Reen relies on both working in concert: `required-features = ["snake_terminal"]` on the bin plus `snake_terminal = ["terminal_renderer"]` in `[features]`. Test that `cargo build --bin snake_terminal --no-default-features` fails with a clear error when neither is provided (Cargo's own error suffices; Reen does not need to intercept it), and succeeds with `--features snake_terminal`.
- **Snake's `src/main.rs` disappears.** The hand-maintained `src/main.rs` at `tests/snake/src/main.rs` will move to `tests/snake/src/bin/snake_terminal.rs` as part of the migration. Preserve every line of the existing logic — terminal setup, raw-mode handling, event loop, the full `run_app_loop` function. The migration is a rename with an added `@generated`-free marker (the file is user-authored, not scaffolded). Scaffold emits a stub at `src/bin/snake_terminal.rs` for a clean project; for the migration, the existing file takes precedence and the stub is skipped by the overwrite-safety rule.
- **Snake web variant requires Phase 10 finished.** If Phase 10 is not merged when you reach the snake-web step, skip that sub-scope, document the gap in the phase's completion report, and flag it as follow-up work. Do not partially land Phase 10 from inside Phase 12.
- **Cargo feature names vs. package names.** Cargo allows features and packages to share names (the `features` block is scoped to the package). No collision handling needed inside Reen; just don't be surprised by it in error messages.
- **Multi-app + web-app.** Phase 10 emits a four-crate workspace for `web_app`. When a project has both `web_app` and `cli_app` apps (snake does, once the migration lands), the workspace layout is Phase 10's (four crates) and the `cli_app` binary lives at `crates/app-server/src/bin/<name>.rs` — **no**, that conflates unrelated shapes. Revised rule: a project with mixed kinds uses Phase 10's workspace layout, and `cli_app` / `tui_app` binaries live at `src/bin/<name>.rs` at the *workspace root*, as a fifth crate named `<project>-cli` (or under a dedicated `crates/app-bins/` crate). Pick one — the `crates/app-bins/` approach keeps the four-crate invariant clean. Confirm the crate layout is emittable and buildable; if it gets ugly, push the `cli_app` binary into the existing `app-server` crate as just another `[[bin]]`. Decide, test, and document the choice.
- **Verify's closure walk performance.** The transitive context graph is small today (<100 nodes per project) so a naive walk is fine. Still, implement it as a breadth-first traversal with a visited set — don't recurse; projects will grow and cycles already exist in some test fixtures (Phase 9's detection caught them, but don't trust).
- **Feature names are BA-facing.** `terminal_renderer`, `web_renderer`, `sandbox_mode` — readable English. Verify's error messages name features by their declared string, not by any internal representation. A BA reading a verify failure should know exactly which feature is missing and where to add it.
- **`reen init --upgrade` dogfood.** The skill and hook updates must survive a round-trip through the bundle. After editing `templates/.claude/skills/*` and `templates/.claude/hooks.json`, run `cargo run -- init --upgrade` and check `git status` is clean. Any diff is a bug in the template-vs-root sync that must be fixed before landing.
- **Error wording stays DCI-language.** Same bar as every prior phase. "`snake_terminal` does not require feature `web_renderer`, but its entry functionality `PlaySnake/run` transitively reaches `WebRenderer`, which is gated by `web_renderer`." Not "feature mismatch: terminal <- web_renderer".

## Finishing the phase

When you believe Phase 12 is done:

1. `cargo test` at the reen workspace root — green.
2. `cargo run -- init --upgrade` in the reen repo — `git status` clean (dogfood).
3. `tests/money transfer` and `tests/Connor`: `reen prepare && reen scaffold && reen verify && cargo check && cargo run` — each runs the app as before. Migration produced no behavioural change.
4. `tests/snake`: `reen prepare && reen scaffold && reen verify` is clean. `cargo build --no-default-features --features snake_terminal` produces a binary that runs the terminal game exactly as before. `cargo build --no-default-features --features snake_web` produces a binary that boots a Phase 10 server and serves the board at `/`. `cargo tree -e features --features snake_terminal` does not include the web-renderer crate graph; `cargo tree -e features --features snake_web` does not include crossterm.
5. A deliberately-broken `tests/snake`: remove `terminal_renderer` from `snake_terminal`'s `Requires Features:` — verify rejects the project with a DCI-language message naming `snake_terminal`, the entry functionality, the reachable gated context, and the missing feature. Restore the line before committing.
6. A fresh fixture project with two apps and three features scaffolds to a workspace where each app builds under its own feature set and neither under `--no-default-features` alone.
7. Two consecutive `reen scaffold` runs produce byte-identical `Cargo.toml` and byte-identical `src/bin/*.rs` files.
8. Update the task list: mark Phase 12 complete.
9. Report back with a one-paragraph summary, the migrated `tests/snake/Cargo.toml` (so the `[features]` and `[[bin]]` sections are visible), and the new `tests/snake/drafts/apps/snake_terminal.md` and `tests/snake/drafts/apps/snake_web.md` in full — they are the canonical small examples for future BA work.
