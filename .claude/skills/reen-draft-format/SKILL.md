---
name: reen-draft-format
description: Draft grammar, section templates, and common pitfalls for Reen drafts
triggers:
  - glob: "drafts/**/*.md"
  - keyword: "reen draft"
  - keyword: "draft format"
---

# Reen Draft Format

Six draft kinds, each with a fixed section structure. No types, no signatures, no Rust tokens.

## Context Draft (`drafts/contexts/<name>.md`)

```
# Context: <Name>
## Purpose          — one paragraph, business terms
## Role Players     — bullet list: `- <role>: <DataType> — <responsibility>`
## Props            — bullet list: `- <name>: <description>` (scalar config only)
## Role Methods     — ### subsections per role, each with `- <method>: <description>` bullets
## Functionalities  — ### subsections, each with prose describing the enactment
## Scenarios        — ### subsections, each with Given:/When:/Then: lines
```

## Data Draft (`drafts/data/<name>.md`)

```
# Data: <Name>
## Purpose          — one paragraph
## Attributes       — bullet list: `- <name>: <description>`
## Invariants       — bullet list of prose invariants
## Operations       — ### subsections with prose descriptions
## Scenarios        — ### subsections with Given:/When:/Then:
```

## Projection Draft (`drafts/projections/<name>.md`)

```
# Projection: <Name>
## Purpose          — one paragraph
## Source           — bullet list referencing underlying data
## Queries          — ### subsections with prose descriptions
## Scenarios        — ### subsections with Given:/When:/Then:
```

## Component Draft (`drafts/components/<Name>.md`)

```
# Component: <Name>
## Purpose          — one paragraph: what this component lets the user see and do
## Props            — bullet list: `- <name>: <data/projection name or prose> — what the prop conveys`
## Slots            — bullet list: `- <slot name>: <what the slot is for>`
## Events           — bullet list: `- <event name>: <what just happened; payload in business language>`
## Variants         — bullet list: `- <variant name>: <when to use this variant>`
## Composition      — (optional) child component bindings and event→context bindings
                      `- <child>: <slot/prop bindings, in prose>`
                      `- when <child>.<event>: invoke <Context>`
## Invariants       — bullet list of prose invariants
## Scenarios        — ### subsections with Given:/When:/Then:
```

Example (coordinating component):
```
# Component: LoginForm

## Purpose
Lets the user enter credentials and submit them for authentication.

## Props
- error_message: a text value shown when authentication fails

## Events
- submitted: the user has submitted their credentials

## Composition
- EmailInput: receives the current email value
- PasswordInput: receives the current password value
- SubmitButton: labelled "Sign In"
- when SubmitButton.pressed: invoke AuthenticateUser

## Scenarios
### successful submission
Given: the user has entered valid credentials
When: the submit button is pressed
Then: the submitted event fires with the credentials
```

## Brand Draft (`drafts/brands/<Name>.md`)

```
# Brand: <Name>
## Purpose          — one paragraph: what experience this brand creates
## Voice            — Tone, Register, words we use/avoid (prose)
## Visual Identity  — ### subsections: Colours, Typography, Spacing
                      Each token: `- <role name>: <description of the role>`
## Motion           — bullet list: `- <token>: <role description>`
## Imagery          — prose style guidelines
## Iconography      — prose rules
## Logo             — usage, clear space, don'ts
```

Example:
```
# Brand: Acme

## Purpose
Creates a professional, trustworthy experience for business users.

## Voice
- Tone: confident but not arrogant
- Register: professional
- Words we use: simple, clear, guide
- Words we avoid: synergy, leverage, disrupt

## Visual Identity
### Colours
- primary-action: the colour that draws the user to the main call-to-action
- surface-default: the base background for content areas
### Typography
- heading-large: the most prominent text on a page
- body-default: standard reading text
### Spacing
- inset-tight: padding inside compact interactive elements

## Motion
- transition-default: standard duration for state changes, feels responsive
```

## Routing Table Draft (`drafts/routes/<name>.md`)

```
# Routes: <name>
## Purpose          — one paragraph: what part of the app this table describes
## Routes           — structured list of routes:
                      `- path: /some/path/<param>`
                      `  component: ComponentName`
                      `  params:`
                      `    param: DataType`
```

**The list form is the only accepted shape.** Each route is a list item starting with `- path:`. The map form (using URL paths as YAML keys, e.g. `/:` or `/login:`) is not supported. This is deliberate: URL path syntax contains characters that conflict with YAML map-key parsing (`<>`, `:`, leading `-`), and the list form is unambiguous regardless of path complexity.

Example:
```
# Routes: main

## Purpose
Maps the primary navigation paths of the application.

## Routes
- path: /
  component: HomeScreen
- path: /products/<id>
  component: ProductScreen
  params:
    id: Product
- path: /orders
  component: OrderListScreen
```

## App Draft (`drafts/app.md`)

Exactly one per project. Describes the application entry point. Three structured fields at the top; the rest is informational prose for the implementer.

```
# The primary application
## Application Kind    — `cli_app`, `tui_app`, or `web_app`
## Entry Context       — PascalCase name of the context the app launches into
## Entry Functionality — functionality on that context invoked at startup (optional if only one)
## Dependencies        — bullet list of crate names the app needs
```

Everything else (Purpose, Startup Sequence, Main Loop Behavior, Collaborators, Error Handling) is preserved as notes prose. It is not schema-enforced — it serves as implementation guidance for Claude Code.

**`web_app` projects** require at least one routing table draft under `drafts/routes/`. Routing is the entry surface for web applications — `Entry Context` and `Entry Functionality` are optional (the routing table maps URLs to components).

Example:
```
# The primary application

## Application Kind
`cli_app`

## Entry Context
`GameLoop`

## Entry Functionality
`tick`

## Dependencies
- anyhow
- crossterm
- chrono
- rand

## Purpose
A terminal Snake game that manages the game lifecycle.

## Startup Sequence
Create a board, snake, food dropper, game state, and GameLoop...
```

## Common Pitfalls

- **Sneaking types into drafts**: No `u32`, `String`, `Vec<>`. Types are inferred during prepare.
- **Mixing role and role player**: "Account withdraws" is wrong. "The source withdraws" is correct — the role does the action.
- **Putting role methods on data**: If a method only makes sense in a specific scenario, it's a role method on the context, not an operation on the data type.
- **Missing scenarios**: Scenarios are required where behaviour is declared (see "When are scenarios required?" below).
- **Mutation in projections**: Projections are read-only. The parser will reject mutation-shaped prose.

## Spec vs. implementation — what does not belong in a draft

> **If the draft needs to name it, it is not an implementation detail.**

Every capitalised type name referenced in a draft (role player types, projection sources, component bindings, route components) must resolve to a declared artefact — a data, projection, context, component, brand, or routing table draft. `reen prepare` and `reen refine` reject unresolved references.

### Implementation-speak patterns

Names ending with any of these suffixes are almost certainly implementation-speak:

`Client`, `Store`, `Sink`, `Ledger`, `Repository`, `Service`, `Manager`, `Handler`, `Adapter`, `Gateway`, `Connection`, `Pool`, `Registry`, `Cache`, `Queue`, `Bus`, `Clock`, `Timer`

These name mechanisms, not domain concepts. The fix is to name what the domain cares about:

| Implementation-speak | Domain rewrite |
|---------------------|----------------|
| `SessionStore` | The user's session is a data concept — author `drafts/data/Session.md` with real attributes and invariants |
| `Clock` | Express time in domain units ("valid for 15 minutes", "expires after the session timeout"). Do not name a time source |
| `AuditSink` | If auditing is a domain requirement, author `drafts/contexts/AuditRecording.md`. If it is cross-cutting infrastructure, omit it from drafts |
| `NordeaClient` | The Nordea integration is a context — author `drafts/contexts/OpenBanking.md` with DCI-native roles. The HTTP client lives inside the context implementation |
| `IdempotencyLedger` | The domain concept is "requests we've already accepted" or "deduplicated attempts" — author a data draft |
| `IncomingRequest` | Author `drafts/data/IncomingRequest.md` with actual attributes (path, headers, session cookie), or rename to the existing domain concept |
| `DateRangeFilter` | Author `drafts/data/DateRange.md`, or use the domain term directly ("the reporting period") |

### Litmus test

Before naming a type in a draft, ask: **"would a non-technical domain stakeholder recognise this name?"** If the answer is no, it is probably implementation-speak. Name what the business cares about; leave the mechanism to the implementer.

## When are scenarios required?

Scenarios track behaviour, not structure. `reen refine` requires scenarios only where behaviour is declared:

| Kind | Scenarios required when… |
|------|--------------------------|
| **Context** | Always — contexts *are* behaviour |
| **Component** | The component declares **Events** or has a **Composition** section. Pure-display components (Props and Slots only) do not need scenarios |
| **Data** | The data draft declares **Operations**. Pure value types (attributes + invariants only) do not need scenarios |
| **Projection** | The projection declares **Queries** (in practice, nearly always) |
| **Brand** | Never |
| **Routing table** | Never |

## Roles vs. props

Every role player must have role methods — that is what makes it a role. If a value is passed into the context but never acted upon through role methods, it is a **prop**, not a role player. Move it to the Props section.

**Role player** (has methods — it *does* something in this context):
```
## Role Players
- source: Account — the account to debit

## Role Methods
### source
- verify_sufficient_funds: checks that the balance covers the amount
```

**Prop** (a passive value — no methods):
```
## Props
- amount: the sum to transfer, a Money value
- idempotency_token: the client-supplied unique token preventing duplicate execution
```

`reen refine` flags role players without role methods: "a role without methods is a prop, not a role."

## What not to put in a draft

`reen prepare` enforces a **code-token lint** that blocks any draft containing implementation-level content. The following families are rejected:

1. **Signature lines** — lines beginning with `Signature:` or backtick-fenced `fn` declarations. Describe what the role method *does*, not its function shape.
2. **Rust path syntax** — `Direction::Up`, `std::io::Stdout`, `rand::rngs::ThreadRng`. Use the domain name ("up", "standard output", "random number generator") instead.
3. **Generic type brackets** — `Vec<Position>`, `Option<Food>`, `Result<()>`, `&[T]`. Describe the shape in prose ("a list of positions", "a food placement if one exists", "succeeds or returns an error").
4. **Reference and mutability sigils** — `&self`, `&mut T`, `Box<T>`, `Arc<T>`. These are ownership concerns, not business language.
5. **Turbofish** — `::<Type>`.
6. **Trait-bound syntax** — `impl Trait`, `dyn Trait`, `where T: Clone`.
7. **Rust primitive types** — `u32`, `i64`, `f64`, `bool`, `char`, `str`, `String`, `usize` etc. Say "a whole number", "a decimal number", "true or false", "a text value" instead.
8. **Method-call syntax** — `state.score()`, `.unwrap()`, `.clone()`. Describe the action ("ask the game state for the score") instead of writing the call.
9. **Macro invocations** — `println!()`, `format!()`, `todo!()`.

**Also rejected:** return-type arrows (`->`), numeric type suffixes (`10_u64`, `1.0_f64`), and cast syntax (`as u64`).

**What is fine:** domain names (`Amount`, `Ledger`, `Account`), concrete values (`250.00 DKK`, `account 123456`), operation names used in prose (`to_str is called`, `withdraw`), and standard English words (`string`, `character`).

## What not to put in a web draft

Web-kind drafts (component, brand, routing table) have **stricter lint rules** with no `--allow-code-tokens` escape hatch:

### Component drafts reject:
- **HTML tag names** — `<div>`, `<span>`, `<input>`, `<button>`, `<form>`, etc. Describe what the user sees ("a text entry field"), not the markup.
- **Style/class attributes** — `style=`, `class=`, `className=`. Appearance is a realisation concern.
- **CSS property syntax** — `color: #fff`, `margin: 10px`, `display: flex`. Visual rules live in brand tokens or a design source.
- **Tailwind/utility class names** — `bg-blue-500`, `text-lg`, `p-4`, `mt-2`. These are implementation tokens.
- **JSX/template syntax** — `{{expression}}`. Components are described in prose.
- **Framework-specific hooks** — `useState`, `useEffect`, `createSignal`, `use_state`, `use_signal`. State management is a realisation concern.
- **Raw colour values** — `#3355ff`, `rgb(50, 100, 200)`, `hsl(210, 50%, 50%)`. Use brand tokens named by role instead.

### Brand drafts reject:
- **Hex/RGB/HSL colour values** — `#ff0`, `rgb()`, `hsl()`. Tokens are named by role, not value.
- **Specific font stacks** — "Helvetica Neue, Arial, sans-serif". Name the typographic role instead.
- **Measurement values** — `14px`, `1.5rem`, `16em`, `100vh`. Spacing and sizing are named by role.

### Routing table drafts reject:
- **Framework-specific route macros** — `get!()`, `post!()`. Routes use the documented path template format only.
- **Regex route patterns** — `Regex(...)`, raw string literals. Paths use `<param>` syntax.
- **Function identifiers as handlers** — `handle_login()`. Handler entries must reference component names.
