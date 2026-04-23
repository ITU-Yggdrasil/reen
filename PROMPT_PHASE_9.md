# Phase 9 implementation prompt

Paste the block below into a fresh `claude code` session at `/Users/rune/projects/reen`. Run `/clear` first so no prior context leaks in.

---

You are implementing Phase 9 of the Reen rewrite — the web-app artefact schema. The authoritative specification is `PLAN.md` in this repository; Phase 9 sits between Phase 8 and Phase 10. Read that section in full before planning. Phases 0–8 are already merged (or queued; work on the latest main regardless).

This is a **schema-only** phase. No codegen. No scaffold changes. No verify changes for the new kinds. Phase 10 picks all of that up.

## Scope (do exactly these)

1. **Three new artefact kinds in the parser.**
   - `ArtifactKind::Component`, `ArtifactKind::Brand`, `ArtifactKind::RoutingTable` in `src/draft_parser.rs`.
   - Directory conventions: `drafts/components/<Name>.md`, `drafts/brands/<Name>.md`, `drafts/routes/<name>.md`. Filename determines kind.
   - Per-kind parse path targeting the templates spelled out in PLAN.md Phase 9.

2. **Prepared schema v3.**
   - Bump `reen.prepare/v2` to `reen.prepare/v3` in `src/prepared.rs` and `src/prepared_contracts.rs`.
   - Component prepared entries: `props[]`, `slots[]`, `events[]`, `variants[]`, `composition` (typed — child component bindings and event→context bindings), `invariants[]`, `synthesised_tests[]`.
   - Brand prepared entries: the token sections as typed groups (colours, typography, spacing, motion) plus human-facing sections (voice, imagery, iconography, logo) as prose.
   - Routing-table prepared entries: `routes[]` with `path`, `component`, `params` (param name → data type, resolved via manifest).
   - v2 and v3 coexist during the transition. Existing v2 consumers keep working; new kinds produce v3. A prepared YAML's top-level `schema` field signals which version it is.
   - **Classification field for contexts.** Add a `realisation_target` field to every context's prepared entry: `client_also`, `server_only`, or `unknown`. Compute it in prepare from the transitive dependency graph per the rules in PLAN.md Phase 10's WASM-inference section. Phase 10 consumes this; Phase 9 only has to compute and store it correctly.

3. **Manifest extensions.**
   - `src/manifest.rs` gains `components[]`, `brands[]` (with a `default` pointer), and `routes[]` sections.
   - Brand tokens are referenceable as `@brand/<token>` (single-brand projects, resolves to the default brand) or `@<brand-name>/<token>` (multi-brand).
   - Component references resolve like any other manifest name.

4. **Reference-graph enforcement in prepare.**
   - Authoritative graph from PLAN.md Phase 9:
     - data → data
     - projections → data + projections
     - operations → data
     - contexts → data + projections + operations + contexts + components + brand + routing table
     - components → data + projections + brand + components + contexts
     - routing table → components + data
     - brand → nothing
   - A data draft that references a context is an ambiguity with a DCI-language message. A component referencing a context is fine. A routing-table route whose `component` isn't in the manifest fails resolution; same for its param types.
   - Add focused tests for each allowed and forbidden edge in the graph.

5. **Concreteness lint extensions.**
   - Extend `src/draft_refine.rs` (or `src/review.rs` if that's where the Phase 3.5 lint lives now) with per-kind rules:
     - Component drafts: reject HTML tag names (`div`, `span`, `input`, `button`, `a`, `img`, `form`, ...), style/class attributes, CSS property syntax (`color: #fff`), Tailwind-style utility class names, JSX/template syntax, framework-specific hook names (`useState`, `useEffect`, `createSignal`, `use_state`, `use_signal`), and raw hex/rgb/hsl colour values.
     - Brand drafts: reject hex/rgb/hsl colour values, specific font stacks ("Helvetica Neue, Arial, sans-serif"), and px/em/rem/vh/vw/pt measurements. Tokens must be named by role, not value.
     - Routing table drafts: reject framework-specific route-matching syntax beyond the documented path template (e.g. no `get!`, no Regex literals); handler entries must reference component names, not function identifiers.
   - No `--allow-code-tokens` escape hatch for the new kinds. These exist to protect the BA-authoring surface for UI work.
   - Unit-test each rule against a known-bad fragment and a known-good fragment.

6. **Skill updates.**
   - `.claude/skills/reen-dci-authoring/SKILL.md` (and its template twin under `templates/.claude/...`): add a section on UI authoring. Cover the component/brand/routing-table triad; the "projection-like vs context-like" vocabulary as an authoring pattern (not a schema distinction); the rule that the draft describes what the user sees and does, not what pixels look like; the convention that pixel-level specificity lives in a linked design source; and the explicit absence of a "store" kind (with a short explanation: data flows through contexts, runtime rebinds live references).
   - `.claude/skills/reen-draft-format/SKILL.md` (and its template twin): add the three new templates with annotated examples, and a "what not to put in a web draft" section enumerating the new lint rules.
   - If Phase 8 has landed, edit `templates/.claude/...` and run `reen init --upgrade` inside the repo so the root `.claude/` stays in sync. If Phase 8 has not yet landed, edit the root `.claude/` directly and note the drift for Phase 8 to reconcile.

7. **Hooks.**
   - `.claude/hooks.json` (and its template twin if present): PostToolUse Write on `drafts/components/**.md`, `drafts/brands/**.md`, and `drafts/routes/**.md` triggers `reen prepare` on the affected draft. Existing `drafts/**.md` rules already cover this pattern; verify the wildcards actually match the new subdirectories and add explicit entries if not.

8. **Tests.**
   - Golden-file tests for minimal fixture drafts:
     - A leaf component (`Button`).
     - A coordinating component (`LoginForm`) with a Composition section that binds a child's event to a context.
     - A brand with at least voice, colours, typography, and spacing sections.
     - A routing table with two routes, one with a path parameter.
   - Reference-graph tests:
     - A component referencing a context resolves cleanly.
     - A data draft referencing a context fails with a DCI-language ambiguity.
     - A routing-table route whose component is absent from the manifest fails with a clear message.
   - Lint tests for each of the new rules, covering both a known-bad fragment that must flag and a known-good fragment that must pass.
   - A classification test: a context whose dependency graph touches only data/projections is classified `client_also`; a context whose graph touches an authenticated operation from Phase 7 is classified `server_only`.

## Out of scope (strictly)

- Scaffold. No codegen emission for components, brand, or routing tables. Phase 10 owns that.
- Verify. The existing verifier continues to cover data / projections / contexts / operations. Do not extend it to the new kinds in this phase.
- WASM anything. Phase 10 consumes the `realisation_target` classification; Phase 9 only computes and stores it.
- Framework choice. Do not commit to Leptos, Dioxus, or anything else. The schema is framework-agnostic.
- A "store" kind. We deliberately rejected this. If a scenario seems to want one, write it with data-flowing-through-contexts value semantics instead.
- A "screen" kind. Screens are components; addressability lives in the routing table.
- Any new subcommand. `reen prepare`, `reen scaffold`, and `reen verify` keep their current CLI surfaces.

## Commit pattern

- `phase 9: add Component, Brand, RoutingTable artefact kinds`
- `phase 9: prepared schema v3 with realisation_target classification`
- `phase 9: extend manifest and reference graph for web kinds`
- `phase 9: extend concreteness lint for web kinds`
- `phase 9: skills and hooks for web kinds`
- `phase 9: golden-file and reference-graph tests`

If a defect in Phases 1–8 surfaces while you work, fix it in its own `phase <n>: fix <defect>` commit before the dependent Phase 9 commit.

## Watchpoints

- **v2/v3 coexistence.** Existing snake, money transfer, and Connor prepared YAML is v2. Do not force-migrate them. Prepare emits v3 only for drafts that involve the new kinds. A workspace mixed-mode test would be a good safety net.
- **Realisation-target classification.** This is the load-bearing Phase 10 handoff. Walk the dependency graph including context→context edges so a client-also context that invokes a server-only context is itself server-only. Be careful with cycles (detect and report as an ambiguity, don't stack-overflow).
- **Brand token naming.** Tokens are named by role, not value. The lint enforces this; the skill explains it; the manifest binds them by name. Watch for BAs (or the migration agent) sneaking value-like tokens in (`blue-500`, `font-size-14`). "background-of-primary-cta" is the right shape.
- **Composition section parsing.** This is the most structured part of the component template. Define a clear subgrammar for event→context bindings ("when `button`.`pressed`: invoke `SubmitOrder`") and fail parsing with a specific message if the BA writes it in an unparseable way.
- **Routing-table param binding.** Path parameters bind to data types. If a param's declared type isn't in the manifest, fail with the same DCI-language ambiguity pattern everything else uses. If a path has `<id>` without a matching entry under `params:`, fail.
- **Lint for component prose specifically.** The lint rules for components are stricter than the general code-token lint. Be careful not to over-flag: "the user types their email" is fine; "an `<input type=email>`" is not. Test borderline cases explicitly.
- **Skill sync with templates.** If Phase 8's templates directory exists, edits go there and the repo root regenerates via `reen init --upgrade`. Do not edit both in the same commit — that's what Phase 8's dogfood check is there to catch.

## Finishing the phase

When you believe Phase 9 is done:

1. `cargo test` at the workspace root — green.
2. `reen prepare` on the four fixture drafts produces valid v3 YAML with the expected structure.
3. `reen prepare` on a deliberately-planted graph violation (e.g. data→context) fails with a DCI-language message that names the offending reference.
4. `reen prepare` on an older v2 test project (snake) still works unchanged.
5. The lint catches each of the new rules in its unit tests.
6. Skills and hooks are updated either at `templates/` (Phase 8 landed) or `.claude/` (Phase 8 not yet).
7. Update the task list: mark Phase 9 complete; leave Phase 10 and Phase 11 pending.
8. Report back with a one-paragraph summary, the shape of the new prepared YAML entries (one representative block per kind), and any follow-ups you recommend before Phase 10 starts.
