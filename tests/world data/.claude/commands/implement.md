---
name: implement
description: Fill todo!() bodies in scaffolded Rust files guided by drafts and tests
---

Implement the `todo!()` bodies in scaffolded Rust files, guided by the draft specification and synthesised tests.

Steps:
1. Read the prepared YAML artifact for the target context/data/projection.
2. Read the corresponding draft markdown for business-level intent.
3. Read the scaffolded `.rs` file to see the stub structure.
4. Fill each `todo!()` body following these rules:
   - Functionalities orchestrate role method calls.
   - Role methods operate on their role player parameter.
   - Data operations implement business rules from the draft.
   - Honour invariants described in the draft.
5. After each method is filled, the PostToolUse hook will run `reen verify && cargo check`.
6. Fix any verification or compilation errors before proceeding to the next method.
7. When all bodies are filled, run `cargo test` to confirm synthesised tests pass.
