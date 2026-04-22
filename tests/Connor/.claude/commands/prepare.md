---
name: prepare
description: Run reen prepare to generate v2 YAML from drafts
---

Run `reen prepare --verbose` to process all drafts into prepared YAML artifacts under `drafts/prepare/`.

If prepare reports blocking ambiguities, surface them in the conversation and suggest using `/refine` to resolve them.

Steps:
1. Run `reen prepare --verbose` in the project root.
2. If exit code is 0, report the number of artifacts prepared.
3. If exit code is non-zero, show the ambiguity messages and suggest resolutions from the prepared YAML's `suggested_resolution` fields.
