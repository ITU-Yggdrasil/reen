# Reen — agent context

This repository implements **Reen**, a DCI-to-Rust build pipeline.
Read this file before touching any source code or draft files.

---

## DCI in this codebase

This project follows **DCI (Data-Context-Interaction)**. The three perspectives map directly to source artefacts:

| Perspective | Question | In Reen |
|-------------|----------|---------|
| **Data** | What the system *is* | Types under `data/`, projections — value objects with narrow APIs |
| **Context** | Which use case is running | `contexts/` types — constructed once per scenario enactment, own role-player fields |
| **Interaction** | What the system *does* | Role methods (private) + functionalities (public) on the context |

### Roles and role players

- A **role** is a **named slot** in the scenario (e.g. `board`, `stdin_source`). It becomes a **field** on the context struct.
- A **role player** is the object instance bound to that slot at runtime. It is a collaborator held by the context — not a separate wrapper type.
- Role players are listed in the **Role Players** section of a draft; **Props** are scalar/config values, not collaborators.

### Role methods — the reen convention

Role methods live **on the context struct**, not on the collaborator types. They are private.

Naming: `<role>_<method>` in `snake_case` (e.g. role `board`, method `at` → `board_at`).

**Signature shape:**
```rust
fn <role>_<method>(&self, <role>_: &<RolePlayerType>, <other params>) -> <Return>
```
- Receiver is always `&self` (the context is never mutated inside a role method).
- The **first explicit parameter** is the role player, named `<role>_` (trailing underscore).
  Use `&mut <RolePlayerType>` only when the body calls a `&mut` method on the player; then receiver also becomes `&mut self`.
- The body delegates: call `<role>_.<method>(other_args)`.

**Calling a role method from a functionality:**
```rust
self.<role>_<method>(&self.<role>, other_args)
```

### Mutability

| Kind | `mutable` field | Default functionality receiver |
|------|-----------------|-------------------------------|
| context | `true` | `&mut self` |
| projection | `false` | `&self` |
| data | `false` | `&self` |

Role methods always use `&self` regardless of the artifact's `mutable` flag.

### Data vs interaction

- Keep long-lived domain rules and structure in **Data** types.
- Keep scenario-specific coordination (sequencing, delegation) in the **context** via role methods and functionalities.
- Do not push use-case-only behaviour onto domain structs.
- Do not add public methods to data types just to satisfy a single use case.

### Spec alignment

The pipeline enforces strict alignment between drafts and generated code:
- Field names = role player / prop names from the spec (sanitised to `snake_case`).
- Private method set = exactly the role methods listed in the spec.
- Public method set = exactly the functionalities listed in the spec.
- Do not add fields or methods beyond what the spec lists unless an agent prompt explicitly permits helpers.

---

## Repository layout

```
src/           Reen library and CLI
  draft_parser.rs   Parses markdown draft files into typed structs
  prepare.rs        Deterministic prepare pass → prepared YAML
  prepared.rs       PreparedArtifact struct (schema reen.prepare/v1)
  fix_agent.rs      LLM-based ambiguity resolver (--fix)
  build_agent.rs    LLM-based implementation writer
  codegen.rs        Generates Rust scaffolding from prepared YAML
tests/
  snake/            End-to-end test project (a DCI snake game)
    drafts/         Markdown spec files
    drafts/prepare/ Prepared YAML artefacts
DCI Primer.md       Detailed DCI reference — read for deeper context
```

---

## Prepared YAML schema (`reen.prepare/v1`)

Key fields on `PreparedArtifact`:
- `mutable: bool` — `true` for contexts, `false` for data/projections
- `roles[]` — role specs with `type` (ValueStatus) and `methods[]`
- `props[]` — prop specs with `type`
- `functionalities[]` — public methods with `signature`, `returns`, `flow`, `extensions`, `guarantee`, `references`
- `ambiguities[]` — blocking and info-level gaps; `blocking` severity prevents build

A `ValueStatus` is resolved when `status` is `"resolved"`, `"defaulted"`, or `"fixed"`.

---

## Full DCI primer

See [`DCI Primer.md`](DCI Primer.md) for the complete conceptual reference including further reading links.
