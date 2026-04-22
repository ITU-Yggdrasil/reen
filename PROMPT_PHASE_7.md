# Phase 7 implementation prompt

Paste the block below into a fresh `claude code` session at `/Users/rune/projects/reen`. Run `/clear` first so no prior context leaks in.

---

You are implementing Phase 7 of the Reen rewrite — external API consumption. The authoritative specification is `PLAN.md` in this repository; Phase 7 sits between Phase 6 and "Testing strategy". Read that section in full before planning.

## Scope (do exactly these)

1. **New artifact kind.** Add `ArtifactKind::Operation` to `src/draft_parser.rs`. Operations are YAML, not prose: deserialise directly via `serde_yaml` into a typed struct. Introduce a parser path for boundary drafts (`drafts/apis/<name>.md`) that extracts `Purpose`, `Source`, `Operations`, `Notes`.
2. **Prepared schema.** Extend `src/prepared.rs` / `src/prepared_contracts.rs` so the `reen.prepare/v2` schema recognises an operation kind. Prepare's job for operations is limited: validate that every referenced data type exists in the api's generated manifest, and surface missing references as DCI-language ambiguities. No test synthesis for operations.
3. **Ingestion command.** Add `reen apis ingest [<api-name>]`. With no argument, ingest all boundary drafts in `drafts/apis/`. Per boundary draft:
   - Load `drafts/apis/<name>.yaml` (committed OpenAPI snapshot).
   - Validate OpenAPI version and basic shape.
   - Apply the operations whitelist from the boundary draft; if absent, include all operations.
   - Emit one `drafts/.apis/data/<api>/<TypeName>.md` per referenced schema, in the existing data-draft markdown template (Purpose cites the OpenAPI `$ref` path; Attributes from properties; Invariants from constraints; Operations and Scenarios empty).
   - Emit one `drafts/.apis/operations/<api>/<OperationName>.yml` per whitelisted operation, using the schema spelled out in PLAN.md Phase 7.
   - Write `drafts/.apis/manifest.yml` with the full set and the OpenAPI snapshot hash.
   - Ingestion must be deterministic: same OpenAPI snapshot → byte-identical output.
4. **Gitignore and hooks.** Add `drafts/.apis/` to `.gitignore`. Extend `.claude/hooks.json` so PostToolUse Write on `drafts/apis/**.md` or `drafts/apis/**.yaml` runs `reen apis ingest <api>`. The Phase 3.5 code-token lint must be exempted on `drafts/.apis/**`; authored boundary drafts (`drafts/apis/<name>.md`) are linted as normal.
5. **Vocabulary manifest.** Extend `src/manifest.rs` so ingested data types and operations are resolvable as `@<api>/<Name>`. During prepare, consult `drafts/.apis/manifest.yml`. Unresolved `@<api>/<Name>` references become DCI-language ambiguities.
6. **Reference-graph extension.** Enforce these rules in prepare:
   - data → data (local or ingested)
   - projections → data + projections
   - contexts → data + projections + operations
   - operations → data (for input/output/error)
   - Operations are NOT referenceable from data or projections.
7. **Scaffold.** Extend `src/codegen.rs` (or a new `src/apis_codegen.rs`) to emit a thin Rust client per operation under `src/apis/<api>/<operation>.rs` with `todo!()` bodies. Struct fields mirror the operation spec (transport + auth + input/output/error types). Public method set: `{ call }` for single/list/none; `{ subscribe, next }` (or similarly named) for `stream_of`.
8. **Verify.** Extend `src/verify.rs` so operation client code is checked against the operation spec: struct fields = spec's declared surface; public method set matches the kind table above. Parser and diff messages in DCI-adjacent vocabulary.
9. **Rebuild world-data.** Use `tests/world data/` as the test case.
   - Rewrite `tests/world data/drafts/apis/aisstream.md` under the new boundary-draft template (subject to the Phase 3.5 lint).
   - Commit `tests/world data/drafts/apis/aisstream.yaml` (the OpenAPI snapshot). If the project already has a local OpenAPI file, promote it to this path.
   - Run `reen apis ingest aisstream` and confirm `drafts/.apis/data/aisstream/` and `drafts/.apis/operations/aisstream/` appear deterministically.
   - Wire a minimal context (or reuse an existing one) that references `@aisstream/<OperationName>` and `@aisstream/<TypeName>` so the full reference graph is exercised end-to-end.
   - Scaffold, implement minimally (just enough that `cargo check` passes and `reen verify` accepts the result), and commit.
10. **Docs touch-ups.**
    - Add a short "External APIs" section to `README.md` explaining the authored-vs-generated split.
    - Add a BA-facing note to `DRAFT_FORMAT.md` that describes only the boundary draft. Generated artifacts are off-limits for authoring.
    - Add a dev-facing note to `.claude/skills/reen-dci-code/SKILL.md` covering the operation scaffold shape and verify expectations.
    - Add a one-paragraph sidebar to `DCI.md` explaining where operations sit in the reference graph.

## Out of scope

- Pagination, batching, retry budgets, rate-limit metadata. These go in `notes[]` on the operation spec. Do not add structured fields for them.
- Migrating any other test project. Snake, money transfer, and Connor are Phase 4 territory and stay as they are.
- Cross-API references (`@aisstream/...` referenced from inside a different api's operation spec). Not needed for world-data.
- `tests/snake_gh/`. Different use case, different phase.
- Touching the v0-archive tag or any previous commits.

## Commit pattern

One commit per logical step where it makes sense:

- `phase 7: add Operation artifact kind and boundary-draft parser`
- `phase 7: add reen apis ingest and deterministic codegen`
- `phase 7: wire api manifest into prepare vocabulary resolution`
- `phase 7: scaffold and verify for operations`
- `phase 7: rebuild world-data under the new pipeline`
- `phase 7: documentation`

If a Phase 1–6 defect surfaces while you work, fix it in its own commit prefixed `phase <n>: fix <defect>` before the Phase 7 commit that depends on it — same pattern Phases 4–6 used.

## Watchpoints

- **Determinism.** Ingestion must be byte-identical on repeated runs against the same OpenAPI snapshot. No HashMap iteration order leaking into file output; no timestamps; no host-specific paths. Add a unit test that asserts ingest-twice-diff-zero.
- **Lint exemption scope.** The lint exemption must apply only to `drafts/.apis/**`, not to `drafts/apis/**`. Confirm by running the lint against a known-bad boundary draft (e.g. one that contains a Rust type name) and asserting it fails.
- **Manifest coherence.** If the whitelist in the boundary draft names an operation that isn't in the OpenAPI yaml, ingestion must fail with a clear message. Same for a data type referenced by an operation but absent from the spec.
- **Reference graph.** Prepare must reject a data draft that references an operation. Prepare must accept a context draft that references an operation. Add focused tests for both directions.
- **OpenAPI coverage.** AisStream's spec uses WebSocket, which isn't a standard OpenAPI transport — there's either a custom extension or a README-level convention in `tests/world data/`. Inspect the existing yaml/json before you start; if the transport is non-standard, encode it in `transport.kind: websocket` and note any unmodelled bits as `notes[]`. Do not invent structured fields for quirks you encounter; `notes[]` is the escape hatch.
- **Manifest vocabulary for ingested types.** `@<api>/<Name>` is the canonical reference form. Decide whether the manifest stores them under a single namespace key per api or as a flat `api/Name` composite key, and be consistent. Update `reen-dci-authoring` if the reference syntax ends up user-visible.

## Finishing the phase

When you believe Phase 7 is done:

1. Run `cargo test` at the workspace root; green.
2. Run `reen apis ingest aisstream` in `tests/world data/`; confirm clean output and `git status` shows no tracked files under `drafts/.apis/` (it is gitignored).
3. Run the full pipeline inside `tests/world data/` (`reen prepare`, `reen scaffold`, `reen verify`, `cargo check`); green.
4. Update the task list in this session: mark Phase 7 complete; note any deferred work as follow-ups.
5. Report back with a one-paragraph summary, the new CLI surface, and any follow-ups you recommend for a Phase 8 (e.g. pagination, cross-api references, repo ingestion).
