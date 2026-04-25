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

5. **Persist the findings durably.** The auditor's response ends with a fenced JSON block tagged `audit-findings`. Extract the JSON content (everything between the fences), write it to a temp file, then run:

   ```
   reen audit record --from <tempfile>
   ```

   The subcommand canonicalises the JSON into `.reen/audit.json`, renders `AUDIT.md` in the project root, and flips tracker entries to `stale` (with the finding text in the `audit_finding` field) for every finding at severity `medium` or above. Findings below the threshold are persisted to `audit.json` and `AUDIT.md` but do not flip tracker entries.

   The flip is idempotent — re-running `/audit` with byte-identical finding text on already-stale entries is a no-op. Findings that cannot be matched to a tracker entry (e.g. because the artefact name is misspelled or the method name doesn't appear in `.reen/build.json`) are still persisted but do not flip; the subcommand reports an `unmatched` count.

   Print the subcommand's summary line so the user sees the flip count.

6. **Do NOT auto-apply changes.** The report is for the user to triage. The next `/implement` invocation will read the tracker and prioritise stale-with-finding entries — that is how the audit-loop closes. If the user wants to fix findings manually, they can edit the relevant draft or Rust file and re-run `reen verify`.

## Example Usage

```
/audit game_loop
/audit Snake
/audit MoneyTransfer
```
