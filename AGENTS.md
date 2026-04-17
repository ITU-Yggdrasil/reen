# Project Instructions

These instructions apply to the entire repository unless a deeper `AGENTS.md` overrides them for a subdirectory.

## Objective

The project is basically a compiler. However there's no programming language but English is used to specify the functionality and the folder structure drives the architecture. The system should be generic, it should_not_ simply make the tests cases in tests/ work.

## DCI First

Before making design or implementation decisions for this repository, read [DCI Primer.md](DCI Primer.md) and treat it as normative guidance for how code, drafts, and contracts should be interpreted.

If a local specification, task, or contract is more explicit than the primer, follow the explicit specification. Otherwise, follow the primer.

Changes that touch `contexts/`, `data/`, drafts, manifests, contracts, or generated Rust should be evaluated against the DCI rules below before editing code.

## Required DCI Rules

Use these rules when making design, naming, and implementation decisions:

- Preserve the distinction between Data, Context, and Interaction.
- Keep long-lived domain structure and stable rules in data types.
- Keep scenario-specific coordination and use-case flow in context types.
- Treat roles as named slots in a use case, not as separate wrapper objects.
- A role player is the object assigned to a role; do not invent extra runtime role objects.
- Role players and props are different concepts and must stay separate.
- Role players are collaborating objects in the scenario.
- Props are fixed inputs to the context that are not role players.
- Context fields for role players and props must match the specification exactly unless an explicit task says otherwise.
- Do not add extra fields, methods, or collaborator APIs just to make a use case fit more conveniently.
- Role methods belong on the context, not on collaborator or data types.
- Role methods should be private unless an explicit spec says otherwise.
- Name role methods as `<role>_<method_name>` in `snake_case`.
- The `<role>` prefix must match the role player field name used by the context.
- Functionalities are the public entry points of the context.
- When reading or implementing a spec, map Role Players to context fields, Props to constructor/context fields, Role Methods to private context methods, and Functionalities to the public context API.
- Use the specification's names consistently. Do not rename roles, props, or functionalities unless the specification itself changes.
- When in doubt, match the specification exactly rather than introducing a cleaner but less faithful design.

## Decision Filter

Before finalizing a change, verify all of the following:

- Is this behavior use-case-specific? If yes, it likely belongs in a context.
- Is this a stable domain fact or reusable domain rule? If yes, it likely belongs in a data type.
- Is this collaborator a role player or just a prop?
- Did I keep role naming aligned with the spec?
- Did I keep role methods on the context and name them with the role prefix?
- Did I avoid inventing wrapper role types or pushing use-case logic into data objects?

If any answer is unclear, re-read [DCI Primer.md](DCI Primer.md) before proceeding.
