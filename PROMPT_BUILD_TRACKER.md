# Build tracker implementation prompt

Paste the block below into a fresh `claude code` session at
`/Users/rune/projects/reen`. Run `/clear` first so no prior context leaks in.

---

You are implementing the incremental build tracker described in
`plans/BUILD_TRACKER.md`. Read that plan in full before doing anything else.
It is the authoritative specification; this prompt only orients you and sets
the guardrails.

## Context

Reen's pipeline today is draft → prepare → scaffold → implement → verify →
audit. The implement phase fills `todo!()` bodies one method at a time. When
a draft or prepared YAML changes, there is no mechanism to identify which
already-implemented methods are still valid — the current workflow forces a
full re-scaffold and full re-implement. This is unacceptable as projects
grow and as we dogfood on lupa.

The build tracker records method-level provenance. On `reen prepare`, it
diffs fragment hashes, cascades staleness through the dependency graph, and
writes `.reen/build.json`. Scaffold becomes AST-aware: preserve `fresh`
method bodies, reset `stale` and `not_implemented` bodies to `todo!()`.
`/implement` reads the tracker and works only on `stale` entries. Verify
flips `stale → fresh` when a method passes. Audit findings can flip entries
back to `stale`. `reen clean` wipes the tracker and all machine-owned
source.

Source under `src/contexts/`, `src/data/`, `src/projections/` is
machine-owned. Users never hand-edit it. The tracker does not detect or
reconcile hand-edits — if a human touches generated source, that is user
error.

## Scope (do exactly these, in order)

Work through the nine phases in `plans/BUILD_TRACKER.md` sequentially. Each
phase is its own commit or small commit series. Do not interleave.

1. **Phase 1 — Tracker data structure.** Create `src/build_tracker.rs` and
   `src/build_tracker/keys.rs`. Implement the schema, load/save, mutation
   API, and canonical key module per the plan. Include serde round-trip
   tests and schema-version gating tests. No integration with prepare or
   scaffold yet.

2. **Phase 2 — Edge extraction.** Extend `src/prepared.rs` and
   `src/prepared_contracts.rs` to emit canonical per-artefact YAML
   fragments (for hashing) and dependency edges per the edge catalogue in
   the plan. Golden tests per edge kind, including at least one
   cross-draft case (e.g. a data type used as a role player in a context
   draft).

3. **Phase 3 — `reen prepare` integration.** Wire the tracker into
   `src/prepare.rs`: hash drafts, short-circuit unchanged files, recompute
   per-artefact hashes and edges, diff, cascade, persist. Print a summary
   of what became stale and why. Integration test using a small in-tree
   fixture: prepare, mutate a single fragment, re-prepare, assert the
   tracker matches the expected state.

4. **Phase 4 — Surgical scaffold.** Rewrite `src/codegen.rs` to be AST-aware
   using `syn`. Match existing methods to tracker entries by canonical key.
   Preserve bodies of `fresh` entries verbatim (byte-for-byte, no
   pretty-printing drift), reset `stale` and `not_implemented` to
   `todo!()`, insert new methods, delete removed methods. Regenerate struct
   fields, imports, and non-method items wholesale. Golden tests per the
   plan's Phase 4 acceptance list.

5. **Phase 5 — `/implement` prompt update.** Edit
   `templates/.claude/commands/implement.md` to read `.reen/build.json` and
   work only on `stale` entries. Update the verify path in `src/verify.rs`
   so that when a previously-stale method passes verify, its tracker entry
   flips to `fresh`. Keep the flip atomic with respect to the PostToolUse
   hook.

6. **Phase 6 — Audit feeds the tracker.** Extend `src/audit.rs` to emit
   structured findings tagged with canonical artefact keys and a severity.
   At or above a configurable threshold, flip the referenced tracker entry
   to `stale` with `reason: "audit_finding"` and attach the finding text.
   Default threshold is conservative.

7. **Phase 7 — `reen clean` + `/clean`.** Add a new CLI subcommand that
   removes `.reen/build.json`, `drafts/prepare/*.yml`, and the reen-owned
   Rust subtrees. Confirm-before-delete; `--force` to skip the prompt.
   Add the matching slash command at
   `templates/.claude/commands/clean.md` so the pipeline has full parity
   with the other phases — it should invoke `reen clean --force` and
   report what was removed. Update both the "CLI" and "Slash commands"
   sections of `templates/CLAUDE.md`. Dogfood with `reen init --upgrade`
   before committing.

8. **Phase 8 — Skills, docs, bundle dogfood.** Add a section to
   `templates/.claude/skills/reen-dci-code/SKILL.md` stating that source
   files are machine-owned and must not be hand-edited. Add a short
   tracker concept note to `templates/CLAUDE.md`. Then run `reen init
   --upgrade` inside the repo and confirm `git status` is clean. Never
   edit the repo-root `.claude/` or `CLAUDE.md` directly.

9. **Phase 9 — Lupa regression.** Point the finished pipeline at lupa.
   Implement end-to-end. Then deliberately mutate one role type and one
   functionality spec, re-run `reen prepare` and `reen scaffold`, and
   confirm only the expected methods are reset. Capture any surprises as
   follow-up issues; do not fix them inside this phase unless they are
   blockers.

## Out of scope

- Drift detection on hand-edited source. The tracker never hashes
  implemented method bodies.
- Tracking verify state as an independent dimension. Verify flips `stale →
  fresh` in place.
- Migration of existing generated source. On first run with the tracker,
  every existing method is treated as `not_implemented` and re-implemented
  once. This one-time cost is accepted.
- Any new pipeline phases. This work is entirely about making the existing
  pipeline incremental.
- Publishing or versioning bumps beyond what the schema version inside
  `.reen/build.json` requires.

## Commit pattern

- `tracker: add build tracker data structure and canonical keys`
- `tracker: emit per-artefact hashes and edges from prepared walker`
- `tracker: wire prepare to diff, cascade, and persist`
- `tracker: ast-aware scaffold preserves fresh bodies`
- `tracker: implement prompt and verify honour the tracker`
- `tracker: audit findings feed the tracker`
- `tracker: reen clean subcommand`
- `tracker: skills, docs, and bundle dogfood`
- `tracker: lupa regression`

If a defect in an earlier phase surfaces while working on a later phase,
fix it in its own `tracker: fix <defect>` commit before the dependent
phase's commit. Do not fold fixes into feature commits.

## Watchpoints

- **Fragment hashing must be canonical.** Serialise YAML with sorted keys
  before hashing. Non-canonical hashing will cause spurious cascades and
  waste downstream effort; test this explicitly.
- **`syn` round-trip fidelity.** Body preservation means the exact token
  stream, not a pretty-printed re-emission. Write a test that scaffolds
  twice with no intervening prepare and asserts byte-identical output.
- **Cross-draft edges.** Resolve sibling references from `drafts/prepare/`.
  If a referenced prepared YAML is missing, emit a clear error naming the
  dangling reference. Do not silently drop the edge.
- **Atomic tracker writes.** The PostToolUse hook can fire mid-scaffold.
  Write `.reen/build.json` via a temp-file-plus-rename so partial state is
  never observable.
- **Conservative audit threshold.** If every audit finding flips its
  artefact to `stale` by default, the first audit on a mature project will
  re-invalidate half the codebase. Default to a threshold that only
  touches actionable findings; make it overridable.
- **Dogfood.** `templates/` is the source of truth. After every templates
  edit, run `reen init --upgrade` inside the repo and verify `git status`
  is clean.
- **Bundle contract from `CLAUDE.md`.** Phase 8 of this work intersects the
  "adding a new artefact kind" checklist (hooks, bundle classification,
  SKILL.md sections). The tracker introduces a new concept, not a new
  artefact kind — you do not need every item on that checklist, but you
  do need to keep `templates/` synchronised and the dogfood test green.

## Definition of done

- `reen prepare` on an unchanged project prints "0 methods stale" and
  produces a `.reen/build.json` identical to the previous run.
- Mutating one role type cascades to every role method on that role, and
  to every functionality/operation/query that calls those role methods,
  and nothing else.
- `reen scaffold` after such a mutation leaves every other method's body
  byte-for-byte unchanged.
- `/implement` with a mixed `stale` / `fresh` tracker fills only the
  stale entries and leaves fresh ones alone.
- Verify passing on a previously-stale method flips the tracker entry to
  `fresh` automatically.
- `/audit` on a project with an issue flips the affected entry back to
  `stale` with the finding text attached.
- `reen clean` returns the project to pre-prepare state without touching
  drafts or hand-written modules.
- Lupa regression: a controlled mutation re-implements only the expected
  methods.
