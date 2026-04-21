---
name: audit
description: Run post-implementation discrepancy audit between draft spec and Rust code
---

Audit the implementation of a DCI artifact against its draft specification.

## Input

The argument is the name of a context, data type, or projection to audit (e.g. `game_loop`, `Snake`, `Account`).

## Steps

1. **Locate the artifact files.** Use the argument to find:
   - The draft markdown under `drafts/` (contexts/, data/, or projections/).
   - The prepared YAML under `drafts/prepare/` (same subdirectory structure).
   - The Rust implementation under `src/` (contexts/, data/, or projections/).
   
   Search all three subdirectories if the kind is ambiguous. If any file is missing, report what's missing and stop.

2. **Read all three files** — draft, prepared YAML, and Rust source. You need the full content of each.

3. **Invoke the reen-auditor agent.** Pass it a prompt that includes:
   - The full path to each of the three files.
   - An instruction to read all three and produce the structured discrepancy report.

4. **Render the report.** Display the auditor's findings as-is — the auditor emits structured markdown with findings that include kind, severity, spec_citation, code_citation, and description.

5. **Do NOT auto-apply changes.** The report is for the user to triage. If they want to fix findings, they can run `/implement` with the audit report attached as context, or fix manually.

## Example Usage

```
/audit game_loop
/audit Snake
/audit MoneyTransfer
```
