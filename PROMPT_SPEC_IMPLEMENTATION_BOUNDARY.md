# Spec / implementation boundary enforcement prompt

Paste the block below into a fresh `claude code` session at
`/Users/rune/projects/reen`. Run `/clear` first.

---

You are enforcing the spec / implementation boundary described in
`plans/SPEC_IMPLEMENTATION_BOUNDARY.md`. Read that plan in full before
doing anything else. It is the authoritative specification; this prompt
orients you and sets guardrails.

## Context

Reen drafts are meant to describe a domain in DCI terms — contexts,
roles, data, projections, components. They must not name implementation
mechanisms like stores, clocks, HTTP clients, audit sinks, idempotency
ledgers, service/repository/manager classes. Today prepare accepts these
leaks silently: the drafts reference types that no artefact declares,
prepare emits YAML with dangling names, scaffold emits Rust that fails
to compile, and the error only surfaces at `cargo check` with a cryptic
"cannot find type X in this scope."

The design axiom is one sentence: **if the draft needs to name it, it is
not an implementation detail.** The fix is three tool changes plus a
cleanup of the first project where the bug was observed (lupa).

## Scope (do exactly these, in order)

1. **Phase 1 — Symbol table and prepare validator.** Build a symbol
   table from all declared artefacts (data, projection, context,
   component, brand, routing table, app, ingested operation) during
   the first parse pass. Walk each draft's role players, attribute
   types, operation parameters, and query return types; resolve every
   referenced name against the table. Unresolved references are hard
   errors with draft path, the referenced name, the role/field using
   it, and a heuristic suggestion from the three categories listed in
   the plan (implementation-speak / missing data draft / missing
   context). Prepare exits non-zero on any unresolved reference. Unit
   tests per suggestion category plus a golden test on a fixture draft
   with a deliberate leak.

2. **Phase 2 — Refine integration.** Extend `src/draft_refine.rs` with
   the same symbol-table lookup. Emit findings with a new category
   (`unresolved_reference` or `implementation_leak`) and the same
   heuristic suggestion logic. Unit tests mirroring Phase 1. Refine's
   slash command surfaces the findings without behavioural change.

3. **Phase 3 — Draft-format skill update.** Add a new section to
   `templates/.claude/skills/reen-draft-format/SKILL.md` titled
   "Spec vs. implementation — what does not belong in a draft." Include
   the axiom verbatim, the implementation-speak pattern list
   (`Client`, `Store`, `Sink`, `Ledger`, `Repository`, `Service`,
   `Manager`, `Handler`, `Adapter`, `Gateway`, `Connection`, `Pool`,
   `Registry`, `Cache`, `Queue`, `Bus`, `Clock`, `Timer`), and concrete
   domain-concept rewrites for the lupa names: `SessionStore` → "the
   user's session"; `Clock` → express time in domain units; `AuditSink`
   → `audit_recording` context if domain, otherwise strip;
   `NordeaClient` → `open_banking` context. End with the litmus-test
   question: "would a non-technical domain stakeholder recognise this
   name?" Dogfood with `reen init --upgrade` and confirm `git status`
   is clean.

4. **Phase 4 — Lupa cleanup.** Lupa lives outside this repo. Ask the
   user for the path before starting. Then for each leaked name:
   - `SessionStore` → rewrite references to use the domain concept the
     session represents. If lupa needs session as a first-class domain
     thing, author `drafts/data/session.md` with real attributes and
     invariants.
   - `IdempotencyLedger` → rewrite to the domain concept (accepted
     requests, deduplicated attempts), likely a data draft.
   - `Clock` → remove references; rewrite temporal logic in domain
     units.
   - `NordeaClient` → author `drafts/contexts/open_banking.md` with
     DCI-native roles. Rewrite every context that invoked
     `NordeaClient` to invoke the new context instead.
   - `AuditSink` → decide with the user whether audit is domain (author
     `drafts/contexts/audit_recording.md`) or cross-cutting (strip from
     drafts). Do not guess.
   - `IncomingRequest`, `DateRangeFilter` → author as data drafts, or
     rename to the already-existing domain concept if one exists.

   After each draft edit, run `reen refine` and `reen prepare` in lupa.
   Both must pass with zero unresolved-reference findings before moving
   to the next draft. When all drafts are clean, run `/clean`,
   `/prepare`, `/scaffold`, `/implement`, `/verify` end to end and
   confirm the project rebuilds.

## Out of scope

- Adding a new draft kind. The whole point of this work is that
  stores, clocks, and clients stay below the spec surface. If a name
  is a real domain concept, it becomes a context or a data draft — not
  a new kind.
- Auto-fixing drafts. Prepare and refine report findings; humans (or
  Claude Code guided by the authoring skill) rewrite the drafts.
- Whitelists or escape hatches to allow known-implementation names.
  There are no exceptions; a name is either declared or rejected.
- Naming style enforcement beyond resolution. If a BA wants
  `ReallyWeirdThing` as a data draft name, that is their call.

## Commit pattern

- `boundary: symbol table and prepare validator for unresolved references`
- `boundary: refine surfaces unresolved references`
- `boundary: draft-format skill documents spec/implementation boundary`
- `boundary: lupa cleanup — <one commit per draft rewrite or new draft>`

Lupa commits happen in lupa's repo, not reen's. Use a matching prefix so
the history reads cleanly.

## Watchpoints

- **Built-in primitives.** `string`, `int`, `bool`, `uuid`, `datetime`,
  and a short explicit list are allowed without declaration. Keep the
  list in code, not as strings scattered through the validator.
- **Ingested operations count as declarations.** They sit in
  `drafts/prepare/operations/` — make sure the symbol table includes
  them so a context referencing an ingested API operation resolves.
- **Error locality.** Every unresolved reference error must name the
  draft path and the role/field using the name. "Unresolved reference
  somewhere in your project" is unacceptable.
- **Heuristic suggestions are hints, never gates.** The validator
  rejects the reference regardless of which category the heuristic
  picked. A BA who insists `NordeaClient` is a domain concept still
  has to author the draft before prepare accepts it.
- **Refine is the first line of defence.** Most BAs will hit the
  refine check before prepare. Invest in finding-quality there —
  specific, instructive, pointing at exactly the line in the draft.
- **Lupa as ongoing regression.** After Phase 4, keep lupa in some
  form of CI (even a periodic dogfood run) so that a regression —
  someone reintroducing `SessionStore` into a draft — fails visibly.

## Definition of done

- `reen prepare` on any draft with an unresolved type reference exits
  non-zero with an error naming the draft, the referenced name, the
  role/field, and a heuristic suggestion.
- `reen refine` surfaces the same issue as a finding before prepare
  ever runs.
- The draft-format skill documents the boundary with concrete
  rewrites for the lupa names.
- Lupa's drafts are clean: zero unresolved references, the open
  banking integration is a proper context, session and related
  concepts are data drafts where they belong, no Clock / Sink / Ledger
  / Store references remain.
- Lupa rebuilds end to end after the cleanup.
