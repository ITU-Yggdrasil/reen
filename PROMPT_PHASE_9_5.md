# Phase 9.5 implementation prompt

Paste the block below into a fresh `claude code` session at `/Users/rune/projects/reen`. Run `/clear` first so no prior context leaks in.

---

You are implementing Phase 9.5 of the Reen rewrite — refinements to two specific rough edges in Phase 9's prose-reference lint and routing-table parser. The authoritative specification is `PLAN.md` in this repository; Phase 9.5 sits between Phase 9 and Phase 10. Read that section in full before planning. Phase 9 is already merged.

This phase has no schema changes. It is pure behaviour-and-UX polish. Small surface, high impact on everyday authoring.

## Scope (do exactly these)

1. **Tighten the prose-reference check (`check_prose_references` or its current name).**
   - **PascalCase-only scan.** Replace the current whole-word regex with `\b[A-Z][A-Za-z0-9]*\b`. Lowercase tokens are context-local (role players, props) and never manifest references.
   - **Exclusion zones — do not scan inside:**
     - backtick-delimited code spans (`` `like this` ``),
     - heading text (the draft's own title lines, `# Component: Button` must not flag `Button`),
     - `Given:` / `When:` / `Then:` lines whose content is a concrete scenario value (example: `Given: a body of [(5,5), (4,5)]` — no references to scan here).
   - **Stopword list.** PascalCase tokens that commonly appear at English sentence starts are never references. Hardcode as a named constant:
     ```
     The, A, An, This, That, These, Those,
     When, If, Then, Given, And, Or, But, So,
     For, On, In, At, Of, With, Without
     ```
     Do not expose this list via config. Extending it is a code change, deliberately.
   - **Two-tier severity.** Split findings into two tiers with distinct behaviour:
     - **High-confidence (blocking):** a reference using the explicit syntax `@<api>/<Name>` or `@brand/<token>` that doesn't resolve; OR a PascalCase token that appears resolved elsewhere in the same draft and also appears in a position where it doesn't resolve (a likely typo of a known reference).
     - **Low-confidence (informational):** a standalone PascalCase token that doesn't appear resolved in the same draft and isn't in the manifest.
     - High-confidence findings go into `ambiguities[]` at the existing severity and fail prepare.
     - Low-confidence findings go into `ambiguities[]` with `severity: low`, print under a clearly-separated "Possible prose references" heading in `reen prepare` output, and never block.
   - **No opt-in or escape hatch.** The tightened rules eliminate the need for a `--skip-prose-check` flag. Do not add one.

2. **Tighten the routing-table parser's error UX.**
   - Keep the list-of-maps form as the only accepted shape:
     ```yaml
     - path: /
       component: HomeScreen
     - path: /products/<id>
       component: ProductScreen
       params:
         id: Product
     ```
   - **Detect map-form attempts up front.** When the routing-table body's first non-blank, non-header line is a path-shaped token followed by `:` (e.g. `/:` or `/login:`), the user attempted path-as-map-key. Fail with this exact message (adjust wording if clearer, but keep the three load-bearing parts — what's wrong, what the correct form is, where to look):
     > *"Routes must be a list of entries, each starting with `- path: <url>`. The map form (`/: { component: ... }`) is not supported — see the reen-draft-format skill for the routing-table template."*
   - Catch generic `serde_yaml` errors inside the routing-table parser and rewrite them into DCI-language equivalents. A BA should never see a `serde_yaml` stack trace.

3. **Update the `reen-draft-format` skill.**
   - Add a dedicated routing-table section that shows the list form, says explicitly that it is the only accepted shape, and explains why briefly (YAML idiom, robust to path syntax including `<>`, `:`, `,`, leading `-`).
   - If Phase 8 has landed, edit `templates/.claude/skills/reen-draft-format/SKILL.md` and run `reen init --upgrade` inside the repo so the root stays in sync. If Phase 8 has not landed, edit `.claude/skills/reen-draft-format/SKILL.md` directly.

4. **Tests.**
   - Prose-reference lint:
     - A PascalCase token that resolves to a manifest entry: passes.
     - A PascalCase token in the stopword list (e.g. "The user types their password"): ignored, not flagged.
     - A PascalCase token inside a backtick code span: ignored.
     - A PascalCase token in a heading line: ignored.
     - A PascalCase token in a concrete scenario value line: ignored.
     - An explicit `@api/Name` that doesn't resolve: high-confidence, blocking.
     - A PascalCase token (`Account`) that appears resolved once and unresolved once (`Accoutn`): high-confidence, blocking, message names the typo.
     - A standalone PascalCase token not otherwise resolved (`Widget` mentioned in one place, not in manifest): low-confidence, informational, does not block.
   - Routing-table parser:
     - A draft using the map form (`/: { component: HomeScreen }` or `/: \n  component: HomeScreen`) fails with the targeted message, not a generic YAML error.
     - A draft using the list form with all path shapes (including `/products/<id>`) prepares cleanly.
   - Regression tests on all existing Phase 9 fixtures: none should start failing, none should start emitting new findings.

## Out of scope (strictly)

- Any schema change. `ambiguities[]` gains the `severity` field at the low-confidence path but the existing high-confidence path is unchanged.
- Adding a prose-check opt-in / opt-out flag. We explicitly decided against this.
- Accepting a second routing-table form. List form is the only form.
- Extending the stopword list via config. Code-only.
- Touching Phase 10 work.

## Commit pattern

- `phase 9.5: tighten prose-reference check (pascal-case, stopwords, exclusions)`
- `phase 9.5: two-tier severity for prose-reference findings`
- `phase 9.5: routing-table parser targeted error message`
- `phase 9.5: update reen-draft-format skill for routing table`
- `phase 9.5: tests and regression coverage`

If a defect in earlier phases surfaces while you work, fix it in its own `phase <n>: fix <defect>` commit.

## Watchpoints

- **Stopword case-sensitivity.** The list is PascalCase at sentence start. Do not also add lowercase variants — the PascalCase-only regex already ignores lowercase. `and` (lowercase) won't match; `And` (capitalised) is in the stopword list and is skipped.
- **Resolved-elsewhere detection for typos.** The "appears resolved once, unresolved once" rule needs a careful implementation: build a per-draft set of resolved PascalCase tokens on the first pass, then use that set on the second pass to upgrade findings from low-confidence to high-confidence. Do not try to do both in one pass — the heuristic needs the full set first.
- **Scenario concrete-value line detection.** The exclusion applies to `Given:` / `When:` / `Then:` lines whose content is recognisably a concrete value (tuples, numbers with units, lists of literals). Be conservative: it's fine for the scanner to still see prose-like Given lines. The rule protects against fixture-like values ("`a body of [(5,5), (4,5), (3,5)]`"), not against all Given/When/Then prose. When in doubt, don't exclude.
- **Error-message stability.** The exact wording of the routing-table error message will be asserted by a unit test. Commit to a specific string; tests depend on it. If you tweak it, tweak the test in the same commit.
- **Skill/template sync.** If Phase 8 is in, edits go in `templates/` and the dogfood check catches drift. If not, edit `.claude/` and leave a note for the next `reen init --upgrade` cycle.

## Finishing the phase

When you believe Phase 9.5 is done:

1. `cargo test` at the workspace root — green.
2. `reen prepare` on a draft that uses `Order` in the English phrase "in order to" — no findings, clean exit.
3. `reen prepare` on a draft with `@aisstream/MissingType` — still fails, high-confidence.
4. `reen prepare` on a draft that resolves `Account` once and mentions `Accoutn` once — fails with a message that names the typo.
5. `reen prepare` on a routing table written in map form — fails with the targeted message, not a YAML stack trace.
6. Re-run `reen prepare` on snake, money transfer, Connor — no new findings compared to pre-Phase-9.5 baseline.
7. Update the task list: mark Phase 9.5 complete; leave Phase 10 / Phase 11 pending.
8. Report back with a one-paragraph summary and a short table showing, for a realistic draft you already have in the repo, the before/after counts of findings by severity.
