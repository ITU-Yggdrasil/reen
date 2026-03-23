# Git-Commit-Backed Incremental Specification Patching

## Objective

Add an optional global mode, `--use-git-commit`, that:

1. snapshots the entire active `drafts/` tree into Git before specification generation
2. records the commit used as the source for each generated specification
3. on later runs, compares the previously recorded source commit to the new draft snapshot commit
4. patches only the relevant parts of an existing specification instead of rewriting the whole file when a trustworthy baseline exists

This mode must also be configurable via `reen.yml`.

Phase 1 covers `create specification`.
Phase 2 reuses the same infrastructure for `create implementation`.

## Feasibility Verdict

This is feasible in the current codebase.

The existing architecture already provides the main building blocks:

- artifact-workspace separation for file vs GitHub backends in `src/cli/artifact_backend.rs`
- per-stage orchestration in `src/cli/mod.rs`
- reusable unified-diff parsing/application in `src/cli/patch_service.rs`
- a proven patch-agent pattern in `src/cli/compilation_fix.rs`

The main work is not the Git plumbing itself. The important design choices are:

- use a managed Git workspace per active artifact workspace
- record the source commit per generated artifact
- treat the recorded source commit as the baseline, not `HEAD^`
- use patch mode only when there is a trustworthy baseline
- keep a full-regeneration fallback for safety

## Important Current-Code Constraints

### CLI/config plumbing

- `src/main.rs` currently wires global CLI flags for `profile`, `verbose`, `dry_run`, and `github`.
- `src/cli/yaml_config.rs` already parses richer `reen.yml` state than `main.rs` actually uses.
- Today, only `github` is resolved from YAML in the main CLI path.

Conclusion:
Adding `use-git-commit` to `ReenConfig` is not enough by itself. New resolution/plumbing is required in `main.rs` and `src/cli/mod.rs`.

### Specification generation path

- `create_specification()` and `create_specification_inner()` live in `src/cli/mod.rs`.
- `process_specification()` always asks the agent for a full specification.
- `write_specification_output()` always writes the full file/body.

Conclusion:
Patch mode is a new execution branch, not a small tweak to the existing write path.

### GitHub backend behavior

- GitHub-backed runs materialize projected files under `.reen/github/<owner>__<repo>/`.
- `WorkspaceContext::artifact_workspace_root()` already exposes the active backend root.
- `GitHubArtifactStore::write_specification()` persists full specification content back to GitHub issue bodies.

Conclusion:
Patch application can happen against the projected file, then the final full content can be written back through the existing store API.

### Patch infrastructure

- `src/cli/patch_service.rs` already parses and applies unified diffs.
- `src/cli/compilation_fix.rs` already demonstrates:
  - agent-generated patch flow
  - guardrails
  - fallback/escalation behavior
  - attempt logging under `.reen/`

Conclusion:
Specification patching should follow the compilation-fix pattern rather than inventing a new patch format.

## Scope

### Phase 1

- global `--use-git-commit` flag
- root-level `use-git-commit: true` support in `reen.yml`
- managed Git snapshotting of the active `drafts/` tree
- per-specification recorded source commit
- patch-mode specification updates for ordinary 1:1 draft -> specification artifacts
- safe fallback to full specification regeneration

## Explicit Phase-1 non-goals

- replacing the existing hash-based build tracker
- introducing a section-level source map between drafts and specifications
- patch-mode support for external API drafts that expand one draft into multiple specification artifacts
- implementation patching

External API drafts should continue to use full regeneration in phase 1, even when `--use-git-commit` is enabled.

## Design Decisions

### 1. Baseline must come from recorded source commit, not previous Git commit

Do not use `HEAD^` as the comparison baseline.

Reason:

- later commits may belong to a different stage
- runs may process only a subset of artifacts
- multiple draft commits may occur before a given spec is regenerated again

Required rule:

- each generated specification records the exact draft snapshot commit it was produced from
- later patching compares `recorded_source_commit -> current_snapshot_commit`

### 2. Use a separate Git metadata store, not the current name-keyed build tracker

Do not store the new commit metadata only in `BuildTracker`.

Reason:

- `BuildTracker` is currently keyed by display name, not stable artifact path
- same file stem in different directories is already a collision risk
- git provenance needs exact source/output paths

Add a new persisted metadata file:

`.reen/git_generation_state.json`

Suggested shape:

```json
{
  "version": 1,
  "specification": {
    "drafts/contexts/game_loop.md": {
      "artifact_workspace_root": ".reen/github/ITU-yggdrasil__snake",
      "repo_root": ".reen/github/ITU-yggdrasil__snake",
      "source_path": "drafts/contexts/game_loop.md",
      "output_path": "specifications/contexts/game_loop.md",
      "source_commit": "abc123...",
      "updated_at": "2026-03-21T10:00:00Z"
    }
  },
  "implementation": {}
}
```

Key rules:

- keys are source paths relative to the active artifact workspace root
- paths are stored exactly as Git pathspecs expect inside the managed repo
- implementation phase will later use the same file under the `implementation` section

### 3. Managed Git workspace rules

Add a small helper module, recommended name:

`src/cli/git_workspace.rs`

It should resolve a `ManagedGitWorkspace` from `WorkspaceContext`.

Required behavior:

#### File backend

- if the active artifact workspace root is inside an existing repo, use that repo
- restrict all staging/commit operations to the active `drafts/` subtree only
- if no repo exists, create a local-only repo at the artifact workspace root

#### GitHub backend

- always use a repo rooted at `artifact_workspace_root()`
- never use an incidental parent repo above `.reen/github/<owner>__<repo>`
- if `.git/` does not exist there, initialize a local-only repo there

This is required so GitHub projections do not pollute the caller's outer repository.

#### Local-only repo identity

For repos initialized by Reen itself:

- set repo-local `user.name=reen`
- set repo-local `user.email=reen@local`

For existing user repos:

- do not mutate identity config
- if commit fails because identity is missing, return a clear error

### 4. Draft snapshot commit algorithm

When `use_git_commit` is enabled and `create specification` starts:

1. resolve the managed Git workspace
2. stage the entire active `drafts/` tree with `git add -A -- <drafts_relpath>`
3. include all files under `drafts/`, not just `*.md`
4. if there are staged changes under that pathspec, create a commit
5. if there are no changes and `HEAD` exists, reuse `HEAD` as `current_snapshot_commit`
6. if there is no commit history yet, the first snapshot commit becomes the baseline-producing commit

Commit message should be deterministic, for example:

`reen(specification): snapshot drafts`

Important:

- commit all draft changes, even if the user requested only specific names
- do not stage or commit generated `specifications/`, `src/`, `tests/`, or unrelated repo files in phase 1

### 5. Patch-mode eligibility

Patch mode is allowed only when all of the following are true:

- `use_git_commit` is enabled
- the draft is not an external API draft
- a git metadata entry exists for that draft path
- the metadata entry points to an existing source commit
- the target specification artifact already exists
- the old draft blob can be read from the recorded source commit

If any prerequisite is missing, use full regeneration.

### 6. No section source map in phase 1

Do not introduce a draft-to-spec section source map in phase 1.

Use this input set instead:

- previous draft content from the recorded source commit
- current draft content from the current snapshot
- unified draft diff between the two commits
- current specification content
- current dependency context

This is enough for a patching agent to update only the affected sections.

If phase-1 patch quality is later inadequate, add section provenance in a future phase. It is not required for the initial version.

## Execution Model For `create specification`

Add a branch in the current specification pipeline:

### Full generation path

Current behavior, used when patch mode is not eligible or patch mode fails.

### Patch generation path

Recommended flow per artifact:

1. derive `source_rel_path` from the draft artifact relative to `artifact_workspace_root`
2. load git metadata entry for that path
3. read `previous_draft_content` from `git show <recorded_commit>:<source_rel_path>`
4. compute `draft_diff` from `git diff <recorded_commit> <current_snapshot_commit> -- <source_rel_path>`
5. read current specification content from the existing artifact
6. send all of that plus current dependency context to a dedicated patch agent
7. require unified diff output that edits only the target specification file
8. apply the diff locally
9. persist the final content through the active artifact store
10. run the existing blocking-ambiguity checks on the patched result
11. update build tracker and git metadata with `current_snapshot_commit`

## Patch agent contract

Recommended new agents:

- `patch_specifications_data`
- `patch_specifications_context`
- `patch_specifications_main`

Mirror the existing category split used by `determine_specification_agent()`.

Rationale:

- the current full-generation prompts are already category-specific
- patch behavior should preserve the same domain-specific constraints

Minimum required inputs:

- `current_draft_content`
- `previous_draft_content`
- `draft_diff`
- `current_spec_content`
- `target_spec_path`
- `direct_dependencies`
- `dependency_closure`
- `implemented_dependencies` when already available

Required output:

- unified diff only
- exactly one target file
- target file must be the existing specification artifact
- no deletions
- no renames

Required prompt rule:

- treat `current_draft_content` as source of truth
- use `draft_diff` to minimize changes
- preserve unaffected sections and wording where possible

### Rejected shortcut

Do not implement phase 1 by:

1. regenerating a full new specification with the existing create agent
2. diffing old spec vs new spec
3. applying that diff

Reason:

- it does not meaningfully optimize model work
- it increases churn because the full-generation prompt can rewrite unrelated sections
- it defeats the stability goal behind patch mode

## Patch application and guardrails

Add a small guardrail layer for specification patches, modeled on compilation fixes.

Required checks:

- patch must touch exactly one file
- touched path must equal the computed target specification path
- no deletions
- no path traversal
- no edits outside `specifications/`

Recommended implementation:

- reuse `parse_unified_diff()` from `src/cli/patch_service.rs`
- add a `check_spec_patch_guardrails(...)` helper in a new module or in `src/cli/mod.rs`

### GitHub metadata comment handling

Projected GitHub specification files include a trailing metadata block.

Phase-1 rule:

- strip the machine metadata block before sending `current_spec_content` to the patch agent
- after patch application, persist through `ArtifactStore::write_specification()`, which will reinsert/update metadata

This avoids needless patch churn in the HTML comment.

## Failure and fallback rules

Patch mode must fall back to full regeneration when any of the following occur:

- missing git metadata entry
- recorded source commit cannot be read
- specification artifact does not exist
- patch agent returns non-diff output
- patch guardrails fail
- diff application fails
- patched content produces blocking ambiguities or otherwise fails validation

Fallback must happen per artifact, not by aborting the whole run immediately.

Recommended verbose output:

- `Using patch mode for <name> (source_commit=<sha>, current_commit=<sha>)`
- `Patch mode unavailable for <name>; falling back to full generation: <reason>`
- `Patch mode failed for <name>; falling back to full generation: <reason>`

## Edge Cases

### No previous commits

If there is no recorded source commit for the artifact:

- do full generation
- record the new snapshot commit after success

This satisfies the requirement that the whole current file is effectively the change set without forcing an unsafe patch-from-nothing flow.

### File not previously tracked in Git

If the file did not exist at the recorded source commit:

- treat it as a new artifact
- use full generation if the target specification does not already exist
- patch mode may be skipped entirely for simplicity in phase 1

### No repository exists

If no repo exists for the active workspace:

- create a local-only repo
- for GitHub backend, create it at `.reen/github/<owner>__<repo>/`
- for file backend, create it at the artifact workspace root

### `--fix` modifies drafts mid-run

If `--fix` patches drafts and retries specification generation:

- the retry must create or reuse a new snapshot commit for the now-updated `drafts/` tree
- the successful run records that later commit

### `--clear-cache`

`clear_cache` should continue to bypass hash-based skipping.

It should not disable git patch mode.

## Phase 2: Implementation reuse

Phase 2 should reuse the same infrastructure:

- same `use-git-commit` flag
- same managed Git workspace
- same metadata store under the `implementation` section
- same recorded-source-commit principle

Recommended phase-2 rule set:

- record the spec snapshot commit used for each generated implementation file
- compare `recorded_spec_commit -> current_spec_commit`
- patch the existing implementation file instead of rewriting it when a trustworthy baseline exists
- always run existing compile + optional auto-fix after implementation patching

Important caution:

Implementation patching is feasible, but it is riskier than specification patching because spec-to-code mapping is weaker. Start with single-target file patching only and keep full regeneration fallback mandatory.

## Recommended Files To Touch

- `src/main.rs`
- `src/cli/mod.rs`
- `src/cli/yaml_config.rs`
- `src/cli/patch_service.rs` if small shared helpers are needed
- new `src/cli/git_workspace.rs`
- new `src/cli/git_generation_state.rs`
- new patch-agent YAML files under `agents/`
- docs updates in `README.md` and `docs/GITHUB_BACKEND.md`

## Acceptance Criteria

1. `reen --use-git-commit create specification` snapshots all draft changes, including untracked files under `drafts/`.
2. `use-git-commit: true` in `reen.yml` enables the same behavior without the CLI flag.
3. GitHub-backed runs use a local-only repo rooted at `.reen/github/<owner>__<repo>/`.
4. Each successful generated specification records the exact source commit used to produce it.
5. A later run compares the recorded source commit to the new snapshot commit, not to `HEAD^`.
6. Eligible ordinary drafts use patch mode and preserve unaffected specification sections.
7. Invalid or unsafe patch attempts fall back to full generation automatically.
8. External API drafts still succeed by using full regeneration in phase 1.
9. Existing blocking-ambiguity behavior still works after patched writes.
10. Existing non-git mode behavior is unchanged when the flag/config is off.

## Test Plan

Add focused tests for:

- YAML parsing and resolution of `use-git-commit`
- managed local repo initialization for file backend
- forced local-only repo initialization for GitHub backend projection roots
- first snapshot commit from untracked draft files
- no-op rerun reusing `HEAD` without creating an empty commit
- metadata recording keyed by relative draft path
- patch-mode happy path for a simple context or data draft edit
- patch guardrail rejection for wrong-path or multi-file diffs
- automatic fallback to full generation when patch mode fails
- `--fix` retry producing a later snapshot commit

## Planning Notes For The Next Agent

Suggested implementation order:

1. add flag/config resolution
2. add managed Git workspace abstraction
3. add persisted git metadata store
4. snapshot draft commits before spec generation
5. add patch-mode eligibility + fallback plumbing
6. add patch agents and guardrails
7. add tests
8. document behavior

Do not start with the patch agent prompts first. The hard dependency is the git baseline/provenance layer.
