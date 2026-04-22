---
name: reen-dci-authoring
description: BA-facing DCI thinking for authoring Reen drafts
triggers:
  - glob: "drafts/**/*.md"
  - keyword: "reen draft"
  - keyword: "DCI"
  - keyword: "scenario"
---

# DCI Authoring for Reen

When helping a BA author or edit a draft, think in DCI vocabulary.

## The Six Artifact Kinds

- **Data** — a domain concept with attributes, invariants, and operations. Example: `Amount`, `Account`, `Board`.
- **Projection** — a read-only view over underlying data. Example: `AccountBalance`. Projections never mutate.
- **Context** — a scenario enactment. Contexts bind role players (data/projections) to roles, define role methods as per-role responsibilities, compose them into functionalities, and specify scenarios with concrete Given/When/Then.
- **Component** — a reusable UI building block. Leaf components (Button, Label) just render; coordinating components (LoginForm, OrderList) compose children and bind events to context invocations via the Composition section.
- **Brand** — identity as one coherent artefact: voice, visual tokens (colours, typography, spacing), motion, imagery, iconography, logo. Machine-consumable tokens are named by role ("primary CTA background"), never by value ("#3355ff").
- **Routing Table** — URL-pattern-to-component binding. Routes map paths to components; path parameters bind to data types from the manifest.

## Key Distinctions

- **Role vs Role Player**: A role is a responsibility in a context ("source"). A role player is the data type that fills that role ("Account"). The same data type can play different roles in different contexts.
- **Role methods**: Per-role responsibilities. They live on the context, not the data type. Written as what the role *does*, not how it is implemented. Example: "withdraw: removes the transfer amount from the source balance."
- **Functionalities**: Named use cases that compose role methods. The prose describes which roles do what in what order.
- **Scenarios**: Concrete enactments with Given/When/Then. They are the primary source of test cases.

## Manifest as Shared Vocabulary

Every data, projection, context, component, brand, and routing-table name must be in the vocabulary manifest (auto-discovered from draft directories). If `reen prepare` flags an unresolved name, either add it to `drafts/vocabulary.yml` or create the corresponding draft.

## UI Authoring

The component/brand/routing-table triad covers web applications:

- **Components as "projection-like" vs "context-like"**: A leaf component is projection-like — it takes props in and renders. A coordinating component is context-like — it composes children and binds their events to context invocations. Both use the same template; the Composition section is what distinguishes them. This is an authoring pattern, not a schema distinction.
- **Drafts describe what the user sees and does, not what pixels look like.** "The user sees a list of orders" is correct. "A `<div>` with `className='order-list'`" is not. Pixel-level specificity lives in a linked design source referenced from the component's Purpose paragraph.
- **Brand tokens are named by role, not value.** "Primary CTA background" is correct. "#3355ff" or "blue-500" is not. The value is a realisation concern for the scaffolder.
- **No "store" kind.** Data flows through contexts. A context takes data in, returns updated data out, and the runtime rebinds live references. The draft vocabulary never names storage — that's a framework concern.
- **No "screen" kind.** Screens are components. Addressability (which URL maps to which screen) lives in the routing table.

## Mini-Example

## App as Entry Point

Every project has exactly one `drafts/app.md` that declares the application's entry context and how it starts. The App is not a DCI artifact — it's the wiring that connects domain artifacts to a running binary. The structured fields (`Application Kind`, `Entry Context`, `Entry Functionality`, `Dependencies`) feed the scaffolder; the remaining prose (Startup Sequence, Main Loop, Collaborators) is implementation guidance for Claude Code.

## Mini-Example

A money transfer context binds two Account role players (source, sink), defines role methods withdraw and deposit on each role, composes them into a "transfer" functionality, and specifies a scenario: "Given source has balance 100, sink has 0, amount is 30 — When transfer — Then source has 70, sink has 30."
