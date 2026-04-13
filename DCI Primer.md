# DCI primer for agents (this repository)

This note aligns **classical DCI** ([Data–Context–Interaction](https://fulloo.info), [Wikipedia: Data, context and interaction](https://en.wikipedia.org/wiki/Data,_context_and_interaction)) with the **terms and code shape** used in Reen specifications and generated Rust code. Use it when reading drafts, contracts, and implementation prompts so naming stays consistent.

---

## What DCI is (short)

Object-oriented systems are **networks of collaborating objects**. DCI makes that collaboration **first-class** by splitting the program into three perspectives:

| Perspective | Question it answers | In Reen |
|-------------|---------------------|---------|
| **Data** | What the system *is* — domain structure and stable facts | Types under `data/`, projection/value objects, etc.: “barely smart” domain objects with narrow interfaces |
| **Context** | Which use case is running and **who plays which role** | A **context** type (often under `contexts/`) that is constructed for one enactment of a scenario and wires collaborators together |
| **Interaction** | What the system *does* in that scenario — the algorithm expressed as **roles talking to roles** | **Role methods** on the context (private), plus **functionalities** (public entry points) |

DCI complements patterns like MVC: MVC often separates presentation; DCI separates **domain shape** (Data) from **use-case behavior** (Context + Interaction).

---

## Role

### In DCI theory

A **Role** is a **named place in the use-case network**. During one enactment of a use case, each Role is bound to **exactly one object** (that object may play **several** Roles in the same context). Roles are **stateless**; they describe responsibilities and messaging, not stored Role-owned state. At runtime, referring to another participant **by Role** (not by concrete domain class everywhere) keeps the interaction readable and matches how people think about the scenario (e.g. `SourceAccount` and `DestinationAccount` in a transfer).

### In this repository

Specs and agents use **role** in two related ways; both trace back to “a Role in the DCI sense”:

1. **Role name** — The identifier for a slot in the scenario (e.g. `source`, `board`, `renderer`). This is the **field name** for that collaborator on the context struct.
2. **Role player** — The **object instance** assigned to that slot (the collaborator). Specifications list **Role Players** with types; the context **holds** those values and orchestrates them.

**Important:** A Role is **not** a separate runtime object beside the domain object. The same physical object is “the thing playing `source`” for this use case. Avoid inventing wrapper types for Roles.

When implementing, **role methods** are named with the **role** (field name) as a prefix, e.g. role `source`, method `withdraw` → `source_withdraw` on the context (see below).

---

## Role method

### In DCI theory

**Role methods** are the small pieces of **use-case logic** attached to Roles. They run **in the context of** the object playing that Role. They may call **other Roles** in the same context (same use-case network) and may call **data-level** operations on `self` (the object playing the current Role) where appropriate. Together they spell out the **interaction** for the scenario.

### In this repository

**Role methods live on the context type, not on collaborator types.**

- They are **private** methods of the context implementation.
- They are the **only** private methods that correspond to the specification’s **Role Methods** section (plus allowed small helpers when the pipeline allows).
- Naming: **`<role>_<method_name>`** in `snake_case`, where `<role>` matches the **role player field name** (e.g. `source_withdraw`).
- They **do not** extend collaborator APIs: do not add public methods to data types just to satisfy a use case; express the use case in the context via role methods and existing collaborator interfaces.
- They are **not** expectations to the role player API

Tracing and mental model: a role method is “what the **`source`** role does in this context for this step,” implemented **on the context** for clarity and encapsulation.

---

## Props

**Props** are **inputs fixed at context construction** that are **not** role players.

- **Role players** are the **objects** participating in the use case (the network’s nodes).
- **Props** are **values or small bundles of configuration** the scenario needs (amounts, limits, labels, handles that are not “playing a named role” in the interaction graph).

In specifications, **Props** and **Role Players** are separate sections. In Rust, both typically become **fields** on the context struct, but the **meaning** differs: role players are **collaborators**; props are **parameters** of the enactment.

**Props MUST match** the specification (names and types). Do not add extra fields beyond what the spec lists for role players and props (unless the implementation agent prompt explicitly allows helpers or additional private state for the same behavior).

---

## How the pieces fit a context spec

Typical context specification sections map as follows:

| Section | Meaning |
|---------|---------|
| **Role Players** | Named roles + collaborator types → **fields** on the context |
| **Props** | Constructor parameters → **fields** |
| **Role Methods** | Private **`<role>_<method>`** methods on the context |
| **Functionalities** | **Public** API of the context (use-case triggers / outcomes) |

The **context** is responsible for **binding** role players to roles (by storing them in the right fields) and for **running** the interaction via role methods and functionalities.

---

## Rules of thumb for agents

1. **Data vs interaction:** Keep long-lived domain rules and structure in **Data** types; keep **scenario-specific** coordination in the **context** (role methods + functionalities).
2. **Roles refer to collaborators:** Use the spec’s **role names** consistently for fields and `role_method` prefixes.
3. **Role methods stay on the context:** Do not push use-case-only behavior onto domain structs unless the spec says otherwise.
4. **Props vs role players:** If it is “an object in the scenario,” it is a **role player**; if it is “a value passed in to configure the enactment,” it is a **prop**.
5. **Read the contract:** This repository enforces strict alignment between specs and code (fields, visibility, method sets). When in doubt, match the specification sections **exactly**.

---

## Further reading

- [FullOO / DCI home](https://fulloo.info) — overview and links to papers and examples.
- [Wikipedia: Data, context and interaction](https://en.wikipedia.org/wiki/Data,_context_and_interaction) — concise definitions of Data, Context, Interaction, roles, and execution model.
