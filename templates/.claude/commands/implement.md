---
name: implement
description: Fill todo!() bodies in scaffolded Rust files guided by drafts and tests
---

Implement the `todo!()` bodies in scaffolded Rust files, guided by the draft specification and synthesised tests.

Steps:
1. **Read the build tracker** (`.reen/build.json`) if it exists.
   - If the tracker exists, work **only** on entries with `state: "stale"` or `state: "not_implemented"`.
   - Skip entries with `state: "fresh"` — their bodies are already valid.
   - If an entry has `reason: "audit_finding"`, read the `audit_finding` text in the tracker entry and use it as additional context when filling the method.
   - If no tracker exists, fall back to filling all `todo!()` bodies.
2. Read the prepared YAML artifact for the target context/data/projection.
3. Read the corresponding draft markdown for business-level intent.
4. Read the scaffolded `.rs` file to see the stub structure.
5. Pick one stale/not_implemented method at a time. Fill its `todo!()` body following these rules:
   - Functionalities orchestrate role method calls.
   - Role methods operate on their role player parameter.
   - Data operations implement business rules from the draft.
   - Honour invariants described in the draft.
6. For `component_render` entries: read the component's draft (`drafts/components/<Name>.md`). The render body must actualise every child listed in the Composition section by constructing it with declared prop bindings and calling `.render()`. It must also enforce every Invariant whose enforcement is visible in the rendered output (e.g. "must not display credential input fields" means no password/text input elements). A render body that paraphrases the Purpose in prose (`<p>Your accounts will appear here.</p>`) is a stub and must be rejected — the body must either reference declared children/props or be converted back to `todo!()`.
7. After each method is filled, the PostToolUse hook will run `reen verify && cargo check`.
   The verify hook will automatically flip the tracker entry from `stale` to `fresh` when the method passes verification.
8. Fix any verification or compilation errors before proceeding to the next method.
9. Stop when the tracker shows no `stale` or `not_implemented` entries (or when all `todo!()` bodies are filled if no tracker exists).
10. When all bodies are filled, run `cargo test` to confirm synthesised tests pass.

## Kind-specific patterns

The scaffolder emits kind-shaped stubs. Follow the pattern that matches the kind of the body you are filling.

**Filling a Data type's `new()` constructor.** The body takes the declared field values, enforces every invariant from the prepared YAML's `invariants[]` list (returning `Err` with a specific message per failed invariant), and constructs `Self`. Do not do additional work — no logging, no audit emission, no side effects, just validate and construct. If an invariant cannot be expressed in the constructor (e.g. cross-instance uniqueness), state so explicitly with a `// invariant <name> delegated to <where>` comment naming the enforcement site, so verify and audit can recognise the delegation.

**Filling a Data type's getter.** Mechanical — return `&self.<field>` for non-`Copy` types, `self.<field>` for `Copy` types. The scaffolder emits the body fully; only touch a getter if the user has manually deleted or modified it. Never add logic to a getter.

**Filling a Data type's `with_*` operation.** Construct a new instance with the field replaced and re-validate via `Self::new(...)` so the same invariant logic applies. Never mutate `&mut self` — return a new `Self` by value, propagating the constructor's `Result`.

**Filling a Projection's functionalities and role methods.** Identical to the context pattern in every respect except mutability. Functionalities take `&self`, walk their declared role methods, and return computed output. Role methods take the role player as `&` (never `&mut`) and return computed output without writing back. The projection itself is referentially transparent — given the same role-player state, the same functionality returns the same output. No logging, no audits, no mutation. If a projection's intended computation requires mutating its source data, the projection is wrong by design — author it as a context instead.

**Filling a Context's functionality.** Unchanged from the existing guidance above. Compose role methods, mutate role-player state through them where needed, and return the computed result.
