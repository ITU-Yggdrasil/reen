---
name: scaffold
description: Generate Rust stub files from prepared YAML artifacts
---

Run `reen scaffold --verbose` to generate Rust source stubs with `todo!()` bodies.

Steps:
1. Ensure `reen prepare` has been run (check that `drafts/prepare/` exists).
2. Run `reen scaffold --verbose`.
3. Report which files were generated.
4. Suggest using `/implement` to fill in the `todo!()` bodies.
