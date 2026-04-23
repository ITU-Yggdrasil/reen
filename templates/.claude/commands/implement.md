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
6. After each method is filled, the PostToolUse hook will run `reen verify && cargo check`.
   The verify hook will automatically flip the tracker entry from `stale` to `fresh` when the method passes verification.
7. Fix any verification or compilation errors before proceeding to the next method.
8. Stop when the tracker shows no `stale` or `not_implemented` entries (or when all `todo!()` bodies are filled if no tracker exists).
9. When all bodies are filled, run `cargo test` to confirm synthesised tests pass.
