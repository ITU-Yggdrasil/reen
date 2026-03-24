# Planning And Semantic-Repair Pipeline

## Objective

Reshape the generation pipeline into a general-purpose staged system that improves behavioral correctness without overfitting to any one test case.

Target stage order:

1. draft
2. specification
3. planning
4. implementation
5. verification
6. semantic repair if needed

The core design principle is to keep these artifacts separate:

- specification: what must be true
- plan: how the pipeline intends to satisfy or repair that contract
- implementation: code that realizes the contract, guided by the plan
- verification: evidence that the implementation still satisfies the contract
- semantic repair: constrained correction that preserves both the contract and the plan

## Feasibility Verdict

This is feasible in the current codebase.

The repo already has the main infrastructure needed:

- stage orchestration in [src/cli/mod.rs](/Users/rune/projects/reen/src/cli/mod.rs)
- agent execution and caching in [src/cli/agent_executor.rs](/Users/rune/projects/reen/src/cli/agent_executor.rs)
- dependency context construction in [src/cli/mod.rs](/Users/rune/projects/reen/src/cli/mod.rs)
- context compaction in [src/cli/pipeline_context.rs](/Users/rune/projects/reen/src/cli/pipeline_context.rs)
- compile-fix patch flow in [src/cli/compilation_fix.rs](/Users/rune/projects/reen/src/cli/compilation_fix.rs)
- patch parsing/application in [src/cli/patch_service.rs](/Users/rune/projects/reen/src/cli/patch_service.rs)
- project-structure generation in [src/cli/project_structure.rs](/Users/rune/projects/reen/src/cli/project_structure.rs)

The largest architectural shift is not adding another agent. It is making planning an explicit pipeline artifact and making semantic repair preserve plan and behavior constraints instead of merely restoring compilation.

## Important Current-Code Constraints

### Specification and implementation are currently adjacent

- `create_specification_inner()` in [src/cli/mod.rs](/Users/rune/projects/reen/src/cli/mod.rs) produces specifications directly from drafts.
- `create_implementation()` in [src/cli/mod.rs](/Users/rune/projects/reen/src/cli/mod.rs) goes directly from specification files to implementation generation.
- There is no dedicated planning stage between specification and implementation today.

Conclusion:

- planning needs to become a first-class stage with its own agent, schema, validation, and persisted report

### The implementation prompt currently assumes inline planning

- [agents/create_implementation.yml](/Users/rune/projects/reen/agents/create_implementation.yml) currently instructs the model to plan during implementation and references `TodoWrite`.
- That is not safe as a provider-agnostic requirement.

Conclusion:

- the implementation prompt must stop depending on provider-specific planning behavior
- the implementation stage should consume a supplied plan artifact instead

### Compile-fix is already an effective late implementation stage

- `ensure_compiles_with_auto_fix()` in [src/cli/compilation_fix.rs](/Users/rune/projects/reen/src/cli/compilation_fix.rs) can materially rewrite generated code.
- This means “implementation quality” is determined by both the implementation agent and the compile-fix agent.

Conclusion:

- semantic repair must become plan-aware and behavior-aware
- verification must run before and after repair

### Context mutability is currently over-constrained

- The prompts currently lean toward context immutability as a default.
- That is reasonable for data types, but too strong for contexts and likely pushes the model into awkward rebuild patterns.

Conclusion:

- immutability-by-default should remain for data types
- contexts should not have a default immutability rule
- context ownership and mutability should be inferred during planning

## Design Decisions

### 1. Keep specification and planning as separate stages

Do not replace specification with planning.

Reason:

- specification is the stable behavioral contract
- planning is the execution strategy against that contract
- collapsing them makes verification weaker because there is no longer a clean distinction between required behavior and chosen implementation strategy

Required rule:

- drafts produce specifications
- specifications produce plans
- plans guide implementation and semantic repair

### 2. Use a generic planning framework with specialized plan kinds

Create one generic planning stage and one shared plan schema, with specialized modes:

- `implementation`
- `semantic_repair`

Reason:

- implementation planning and repair planning share structure
- but they optimize for different things
- implementation is constructive
- repair is conservative and scope-minimizing

Required rule:

- one planning engine
- one shared schema
- plan-kind-specific required fields and validation

### 3. Contexts are not immutable by default

Keep immutability-by-default only for data types.

For contexts:

- do not assume immutable by default
- do not assume mutable by default
- infer ownership, identity, and mutation semantics from behavioral hints unless the specification explicitly constrains them

Behavioral hints include:

- shared
- same instance
- one stream
- process lifetime
- reused
- must not replace
- buffer
- session state

Required rule:

- these hints become planning constraints, not fixed Rust signatures in the specification

### 4. Ownership and borrowing awareness belongs in planning and implementation

Do not encode Rust borrowing mechanics in the specification unless the specification explicitly requires them.

Instead, planning should classify important collaborators and state using semantic categories, for example:

- `identity_semantics`
  - `owned`
  - `shared_identity`
  - `replaceable`
  - `recreated_per_operation`
  - `immutable_value`

- `mutation_semantics`
  - `immutable`
  - `mutable_in_place`
  - `internally_mutable`
  - `returns_updated_value`
  - `shared_mutable`

- `rust_guidance`
  - for example: “preserve one logical stream instance”, “avoid independent clones”, “prefer borrowed read-only access”

Required rule:

- “shared” is a semantic hint, not a direct instruction to use references
- the implementation agent should choose the most idiomatic Rust mechanism consistent with the plan

### 5. Semantic repair must preserve both the plan and the contract

Compile-fix should no longer be treated as “make it compile at any cost.”

Required rule:

- semantic repair may fix compilation or integration problems
- it may not degrade previously satisfied behavioral obligations
- if the only possible fix is a semantic stub or a loss of required behavior, the repair stage must fail and escalate

## Scope

### Phase 1

- add a dedicated planning stage between specification and implementation
- create a shared plan schema
- add a provider-agnostic planning agent
- make implementation consume validated plans
- remove provider-specific planning assumptions from implementation prompts
- relax context immutability defaults
- make verification plan-aware

### Phase 2

- add semantic-repair planning
- make compilation-fix consume a `semantic_repair` plan
- add stronger post-repair semantic regression checks

### Explicit non-goals for the first cut

- replacing the specification stage with planning
- formal proof of semantic correctness
- fully provider-specific optimized plans
- language-agnostic ownership models beyond what the current Rust-oriented pipeline needs

## Planned Artifacts

### 1. Behavior contract

Produced from the specification stage.

Purpose:

- normalize key required behaviors into a structured verification surface

Suggested fields:

- target spec path
- title
- specification kind
- required collaborators
- required env/config inputs
- required outputs
- required lifecycle/setup/teardown requirements
- delegation requirements
- role-method names
- shared-state clues
- external behavior clues

### 2. Generic plan

Produced by the planning stage.

Shared fields:

- `plan_kind`
- `target_spec_path`
- `target_output_paths`
- `required_behaviors`
- `required_collaborators`
- `cross_component_integrations`
- `identity_and_sharing_constraints`
- `mutation_constraints`
- `ordered_tasks`
- `verification_targets`
- `risks`
- `forbidden_regressions`

#### Implementation plan additions

- file creation/update strategy
- dependency artifacts to consult
- required integration edges
- expected API surface
- ownership/borrowing guidance

#### Semantic-repair plan additions

- failing diagnostics
- suspected root cause
- minimal edit scope
- touched files
- invariants that must not regress
- post-repair checks

### 3. Implementation evidence / self-audit

Purpose:

- map obligations to concrete evidence in generated code

Suggested fields:

- obligation id
- satisfied
- evidence paths/symbols
- notes

### 4. Verification reports

- specification lint report
- plan validation report
- static behavior verifier report
- semantic regression report

## Agent Changes

### New planning agent

Add a dedicated planning agent, recommended file:

- [agents/create_plan.yml](/Users/rune/projects/reen/agents/create_plan.yml)

Alternative naming is fine, but one generic planning agent is preferred over separate implementation-only and repair-only planners.

Prompt requirements:

- provider-agnostic
- no `TodoWrite`
- no assumptions about provider-specific planning tools
- output must be structured and machine-validated

Inputs:

- specification content
- dependency context
- behavior contract
- `plan_kind`

Outputs:

- structured plan only

### Specification agent changes

Files:

- [agents/create_specifications_context.yml](/Users/rune/projects/reen/agents/create_specifications_context.yml)
- [agents/create_specifications_main.yml](/Users/rune/projects/reen/agents/create_specifications_main.yml)

Required changes:

- remove context immutability as a default rule
- keep data immutability as a default rule where appropriate
- say explicitly that context mutability and ownership remain implementation/planning concerns unless the draft specifies them
- preserve sharing/identity hints as behavior, not implementation mechanics

### Implementation agent changes

File:

- [agents/create_implementation.yml](/Users/rune/projects/reen/agents/create_implementation.yml)

Required changes:

- remove hard `TodoWrite` requirement
- consume `input.implementation_plan`
- add fallback wording only for missing-plan scenarios
- remove context immutability-by-default rule
- keep data immutability-by-default rule
- require idiomatic ownership and borrowing choices consistent with the supplied plan
- forbid semantic stubs for required behavior

### Semantic repair agent/context changes

File:

- [agents/resolve_compilation_errors.yml](/Users/rune/projects/reen/agents/resolve_compilation_errors.yml)

Required changes:

- consume behavior contract and semantic-repair plan
- preserve required collaborators, required outputs, and shared-state invariants
- refuse fixes that replace required behavior with placeholders

## Codebase Changes

### 1. Add planning orchestration

Primary file:

- [src/cli/mod.rs](/Users/rune/projects/reen/src/cli/mod.rs)

Required work:

- insert planning between dependency-context assembly and implementation generation
- for each implementation target:
  - build dependency context
  - read specification content
  - derive or load behavior contract
  - call planning agent with `plan_kind = implementation`
  - validate plan
  - persist plan report
  - pass plan into `create_implementation`

Later extension:

- before semantic repair, call planning agent with `plan_kind = semantic_repair`

### 2. Add plan schema and validation

Recommended new module:

- `src/cli/planning.rs`

Responsibilities:

- plan schema
- parsing
- validation
- JSON/report serialization
- integration helpers for implementation and repair stages

Validation rules:

- target output path must exist or be constructible
- required collaborators must be accounted for
- required behaviors must map to tasks
- verification targets must exist
- plan kind must match downstream use
- no missing task ordering for critical obligations

### 3. Extend quality/verification support

Primary file:

- [src/cli/pipeline_quality.rs](/Users/rune/projects/reen/src/cli/pipeline_quality.rs)

Required work:

- keep behavior-contract extraction here or split into a dedicated module if it grows further
- add plan-aware validation helpers
- add checks that implementation evidence satisfies plan obligations
- add checks for shared identity / clone misuse when the plan requires stable shared identity

### 4. Update compile-fix integration

Primary file:

- [src/cli/compilation_fix.rs](/Users/rune/projects/reen/src/cli/compilation_fix.rs)

Required work:

- add semantic-repair planning call before patch generation
- pass repair plan into compile-fix context
- preserve current semantic regression comparison machinery
- extend regression checks to compare plan-relevant invariants, not only verifier deltas

### 5. Update agent input plumbing if needed

Primary file:

- [src/execution/agent_input.rs](/Users/rune/projects/reen/src/execution/agent_input.rs)

Required work:

- no schema change is strictly required because arbitrary additional context already flows through
- but add explicit support/documentation for:
  - `behavior_contract`
  - `implementation_plan`
  - `semantic_repair_plan`

### 6. Update embedded agent registration if adding a new agent

Primary files:

- [src/registries/embedded_agent_assets.rs](/Users/rune/projects/reen/src/registries/embedded_agent_assets.rs)
- agent model registry YAML files under [agents/](/Users/rune/projects/reen/agents)

Required work:

- register new planning agent
- choose model defaults
- decide whether planning can run in parallel

## Verification Changes

### Specification lint

The specification stage should now fail on:

- internal contradictions
- contradictions between dependency-resolved content and summary sections
- unresolved collaborator references
- env/config contradictions
- accidental waiver of required behavior
- role/method coverage gaps

### Plan validation

New gate between planning and implementation:

- fail early if the plan is incomplete or contradictory

### Static behavior verification

Before accepting generated code:

- run static verifier
- require implementation evidence/self-audit if needed
- block high-risk placeholder patterns in behavior-sensitive code

### Post-repair regression verification

Before applying repair:

- capture verifier state on touched files

After applying repair:

- rerun verifier
- compare before/after
- restore backups and fail if semantic quality worsened

## Environment Variable Detection

Env-var extraction and linting should remain strict but contextual.

Required rules:

- uppercase tokens are not env vars just because they are uppercase
- ordinary acronyms like `JSON`, `ANSI`, `ASCII`, `HTTP`, `FIFO`, etc. must not be treated as env vars by default
- casing examples like `OBSTACLE` in prose must not count as env vars
- quoted or backticked tokens only count if the surrounding line is actually talking about environment/config behavior
- explicit `NAME=value` forms should still count

This must remain a general-purpose rule, not a special case for any single spec file.

## Rollout Plan

### Phase 1: Prompt and semantic policy cleanup

1. remove provider-specific planning assumptions from implementation prompt
2. remove context immutability default from specification and implementation prompts
3. retain data immutability default
4. add ownership/borrowing-aware guidance to planning and implementation prompts

### Phase 2: Add generic planning stage

1. add planning agent
2. add plan schema and validation
3. integrate planning into `create_implementation()`
4. persist planning artifacts under `.reen/pipeline_quality/`

### Phase 3: Plan-aware implementation

1. make implementation consume validated plans
2. add implementation evidence/self-audit
3. strengthen verifier to check plan obligations

### Phase 4: Plan-aware semantic repair

1. add `semantic_repair` plan kind
2. call planner before repair
3. feed repair plan into compile-fix
4. block plan regressions as well as verifier regressions

## Success Criteria

The redesign is successful when:

- specifications remain stable behavioral contracts
- planning is explicit, inspectable, and provider-agnostic
- implementation no longer depends on provider-specific planning behavior
- contexts are not forced into immutability unless the specification says so
- data types remain immutable by default unless specified otherwise
- ownership and borrowing choices are deliberate and idiomatic
- semantic repair cannot silently hollow out required behavior
- verification rejects “compiles but behaviorally broken” outputs

## Recommended File Set To Touch

### Agents

- [agents/create_specifications_context.yml](/Users/rune/projects/reen/agents/create_specifications_context.yml)
- [agents/create_specifications_main.yml](/Users/rune/projects/reen/agents/create_specifications_main.yml)
- [agents/create_implementation.yml](/Users/rune/projects/reen/agents/create_implementation.yml)
- [agents/resolve_compilation_errors.yml](/Users/rune/projects/reen/agents/resolve_compilation_errors.yml)
- new: [agents/create_plan.yml](/Users/rune/projects/reen/agents/create_plan.yml)

### CLI / orchestration

- [src/cli/mod.rs](/Users/rune/projects/reen/src/cli/mod.rs)
- [src/cli/pipeline_quality.rs](/Users/rune/projects/reen/src/cli/pipeline_quality.rs)
- [src/cli/compilation_fix.rs](/Users/rune/projects/reen/src/cli/compilation_fix.rs)
- new: `src/cli/planning.rs`

### Registry / agent loading

- [src/registries/embedded_agent_assets.rs](/Users/rune/projects/reen/src/registries/embedded_agent_assets.rs)
- agent model registry YAMLs in [agents/](/Users/rune/projects/reen/agents)

### Optional supporting updates

- [src/execution/agent_input.rs](/Users/rune/projects/reen/src/execution/agent_input.rs)

## Final Recommendation

Implement this in the following order:

1. prompt cleanup and semantic-policy cleanup
2. generic planning stage
3. plan-aware implementation
4. plan-aware semantic repair

That order yields the best quality improvement with the least disruption to the existing pipeline.
