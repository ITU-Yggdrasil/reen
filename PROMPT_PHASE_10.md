# Phase 10 implementation prompt

Paste the block below into a fresh `claude code` session at `/Users/rune/projects/reen`. Run `/clear` first so no prior context leaks in.

---

You are implementing Phase 10 of the Reen rewrite — Rust SSR codegen with WASM target inference for the web-app schema introduced in Phase 9. The authoritative specification is `PLAN.md` in this repository; Phase 10 follows Phase 9.5 and Phase 2.5 (both of which must be merged). Read the Phase 10 section in full before planning. Phases 0–9.5 and Phase 2.5 are merged.

This is the largest new engineering surface since Phase 2, because the scaffold shape is load-bearing for every web project Reen will ever produce. Take the design seriously and don't take shortcuts — a wrong shape here is a lot of rework later.

## What Phase 10 is and is not

*Is:* extend `reen scaffold` and `reen verify` to handle `Component`, `Brand`, `RoutingTable` artefact kinds (from Phase 9) and the `web_app` variant of the `App` kind (from Phase 2.5); consume the `realisation_target` classification (computed in Phase 9) to pick between WASM compilation and RPC edges; lay out a four-crate Cargo workspace that SSR and WASM-client builds share.

*Is not:* committing Reen to a specific rendering framework (Leptos, Dioxus, Askama, Maud, Axum+raw — the agent picks in `/implement`, Reen stays agnostic); synthesising runtime logic beyond boilerplate; publishing a crate; adding a `screen` kind (screens are components bound to routes); adding a `store` kind (data flows through contexts, runtime rebinds live references); extending Phase 9's schema (v3 is frozen for this phase); changing the prepared-schema version.

## Scope (do exactly these)

1. **Extend the `App` kind with `web_app`.**
   - In `src/draft_parser.rs`, allow `Application Kind: web_app` (previously rejected in Phase 2.5).
   - A `web_app` App entry requires a declared `RoutingTable` in the same project (ambiguity with a DCI-language message if missing).
   - `Entry Context` / `Entry Functionality` remain optional for `web_app` — routing is the entry surface, not a single context call. A declared `Entry Context` means "mount this at `/` if not covered by the routing table" rather than "call this on startup."
   - Preserve Phase 2.5's invariant: exactly one App per project.

2. **Workspace layout.**
   - For `web_app` projects, `reen scaffold` emits a four-crate workspace. Existing single-crate scaffold stays the path for `cli_app` / `tui_app` — do not retrofit CLI/TUI projects to the workspace layout.
   - Layout:
     ```
     <project>/
       Cargo.toml                 # workspace root
       crates/
         app-core/                # data, projections, client-also contexts
         app-server/              # server-only contexts + operations; server binary entry
         app-client/              # WASM build; client-side handlers bound to components
         app-components/          # components, brand, routing — shared client + server
     ```
   - `app-core` is framework-agnostic value logic. No rendering, no HTTP. Builds to both native and WASM cleanly.
   - `app-server` depends on `app-core` and `app-components`. Contains contexts whose `realisation_target` is `server_only` and all operations (operations are server-side by default — Phase 7 authenticated endpoints are server-side by construction). Binary entry `src/main.rs` here.
   - `app-client` depends on `app-core` and `app-components`. Contains client-side event handlers that invoke contexts. Builds to WASM. A `--no-client` scaffold flag skips this crate entirely for server-only projects; in that case, component event handlers target the server via form POSTs rather than WASM.
   - `app-components` depends on `app-core` only. Contains component structs, the `Router`, and the brand module. No server-only logic here — this crate has to build for WASM.
   - Workspace `Cargo.toml` lists the four crates as members. Root `Cargo.toml` is workspace-only; the existing `[package]` / `[[bin]]` pattern from Phase 2.5 does not apply — the binary lives in `app-server`.
   - Emit a `rust-toolchain.toml` pinning a stable channel (whatever `Cargo.toml` uses today) and setting `targets = ["wasm32-unknown-unknown"]` when `app-client` is present.

3. **Per-kind codegen.**

   *Components.* One `.rs` file per component under `crates/app-components/src/components/<name>.rs`.
   - Public struct with fields matching declared `props[]` (typed via manifest).
   - Ambient brand reference as a `&'static BrandTokens` field (or equivalent for multi-brand projects).
   - Public `render(&self) -> Markup` method. `Markup` is a newtype wrapping `String` defined once in `app-components::markup`. The body is `todo!()` — rendering strategy is the implementer's choice.
   - One public method per declared event: `fn on_<event>(&self, <args>) -> EventOutcome` where `EventOutcome` is an enum in `app-components` with variants for each kind of follow-up the Composition section declared (invoke a context, emit an event to a parent, navigate, no-op). Bodies are `todo!()`.
   - Composition section produces typed child-component binding metadata and event→context routing as generated code (a `dispatch` helper that takes a parsed event and routes to the right context call).
   - Variants become an enum on the component struct.

   *Brand.* One module under `crates/app-components/src/brand/<name>.rs`.
   - Colour tokens: `pub const <TOKEN_NAME>: &str = "<css-var-name>";` and a companion `pub static CSS: &str = include_str!("<name>.css");` where `<name>.css` is a scaffolded sibling file containing the `:root { --<token>: ... }` declarations as placeholders the implementer fills. Reen does not invent hex values.
   - Typography, spacing, motion: typed structs (`struct TypographyScale { body: &'static str, heading: &'static str, ... }`) with named roles and `"<to-be-filled>"` placeholders in the scaffold.
   - Voice, imagery, iconography, logo sections ride as `#![doc = "..."]` module docs. Implementers read them; compiler ignores them.

   *Routing table.* One file at `crates/app-components/src/router.rs`.
   - A `Router` struct with one method: `fn dispatch(&self, path: &str) -> Result<Route, RouterError>` where `Route` is an enum with one variant per declared route carrying its decoded params.
   - Path-parameter decoders: `<id>: Product` in the draft becomes a call to `Product::decode(segment)` where `Product` is whatever the manifest-resolved type supplies as a `FromStr` impl (error on missing `FromStr` — the implementer has to add one).
   - A companion `fn render(&self, route: Route) -> Markup` method that dispatches to the right component's `render()` with the decoded params bound to the component's props. Bodies of the match arms are deterministic from the routing table + component specs; emit them fully.

4. **WASM target inference and RPC edges.**
   - Consume the `realisation_target` field computed in Phase 9 (`client_also` | `server_only` | `unknown`). `unknown` is a Phase-9 error; do not re-derive it here.
   - A context in `app-core` if classified `client_also`; in `app-server` if `server_only`. An `unknown` classification at scaffold time is a bug — fail loudly with the context name and the unresolvable transitive dependency.
   - When a component's Composition section invokes a `server_only` context, emit an RPC edge:
     - `app-server` gets a route `POST /_rpc/<context-snake>/<functionality-snake>` whose handler deserialises the arguments, calls the context, serialises the result. Body is `todo!()`; the contract (route path, request/response types) is deterministic.
     - `app-client` gets a stub `fn <context-snake>_<functionality-snake>(<args>) -> impl Future<Output = Result<<ret>, RpcError>>` that `fetch`es the endpoint. Body is `todo!()`.
     - `app-components` gets an `EventOutcome::InvokeRemote { endpoint, payload }` variant that the component's `on_<event>` method returns; the actual HTTP call happens in `app-client` (WASM) or in a form-POST flow (no-WASM builds).
   - For `client_also` contexts, the component's `on_<event>` method invokes the context directly via normal Rust call — no RPC edge, no WASM/server split needed at the call site (the whole context compiles to both targets).
   - Serde derives: Reen does **not** globally add `Serialize` / `Deserialize` to every data type. It adds them only to the specific types that appear as inputs or outputs on an RPC edge, via an additive derive emission in the affected data module. Preserve existing derives; don't duplicate.

5. **Server binary scaffold.**
   - `crates/app-server/src/main.rs` is generated and contains, at a minimum:
     - A `main` function that boots an HTTP server on a configurable port (env var, fallback `3000`).
     - Route table: one handler per routing-table entry calling `Router::render(&route) -> Markup` and returning the HTML.
     - One handler per RPC edge calling into the server-only context.
     - Static-asset route for the generated stylesheet and (if `app-client` is present) the WASM bundle.
   - The HTTP crate (axum, warp, tide, actix) is **not** chosen by Reen. The scaffolded `main.rs` uses placeholder calls into a trait `HttpServer` with `todo!()` implementations, which the implementer binds to their framework of choice. This keeps Reen framework-agnostic while still emitting a deterministic route-mount skeleton.
   - Alternative that is fine: emit a `main.rs` that uses `std::net::TcpListener` + a tiny dispatch function, leaving framework adoption as a refactor the implementer can choose. Pick whichever is easier to verify — document the choice in the generated file's `@generated` comment.
   - The `@generated` marker convention from Phase 2.5 applies — the implementer can take ownership by removing the marker line.

6. **Client WASM scaffold.**
   - `crates/app-client/src/lib.rs` is the WASM entry.
   - For each component with a `client_also` context binding, emit a `#[wasm_bindgen]` init function that wires DOM event listeners to the component's `on_<event>` methods. Bodies are `todo!()`.
   - For each RPC edge, emit the `fetch`-based stub described above.
   - `Cargo.toml` for `app-client` sets `crate-type = ["cdylib", "rlib"]` and declares `wasm-bindgen` as a dependency. Do not choose a specific WASM build tool (trunk, wasm-pack, dioxus-cli) — the README explains the options.

7. **Verify extensions.**
   - New `reen verify` cases for components, brand, routing tables.
   - Component: struct fields match `props[]`; `render` exists and returns `Markup`; each event has an `on_<event>` method with the right signature.
   - Brand: constants exist with the declared names; the companion CSS file exists.
   - Routing table: `Router::dispatch` match exhausts the declared routes; each route's param types align with manifest.
   - WASM-classification sanity check: walk the crate graph. A `client_also` context must not depend on a symbol from `app-server`. Flag as drift in DCI language ("the context `ValidateAddress` is classified client-also but depends on `AuthenticatedPayment`, which is server-only").
   - RPC edge existence: for every component Composition binding to a `server_only` context, both the server handler and the client stub must exist.

8. **CLI.**
   - No new subcommands. `reen scaffold` and `reen verify` get extended; their CLI surfaces are unchanged. Same for `reen prepare`.
   - `reen scaffold --no-client` flag for server-only projects (skips `app-client`).
   - `reen scaffold --force-workspace` is tempting but out of scope — don't add it.

9. **Hooks.**
   - PostToolUse Write on any of the new crate paths (`crates/app-*/src/**.rs`) triggers `reen verify && cargo check -p <crate>` for that specific crate. The existing generic `src/**.rs` hook still applies for single-crate CLI projects; for workspaces, the per-crate pattern is stricter and faster.
   - Edit in `templates/.claude/hooks.json` and run `reen init --upgrade`.

10. **Skill updates.**
    - `templates/.claude/skills/reen-dci-code/SKILL.md`: add a web-app section covering the four-crate layout, where each kind lives, the `Markup` newtype, how `on_<event>` methods are expected to look, what the RPC edges compile down to, and the rule that frameworks live in bodies (not in Reen-owned scaffolding).
    - `templates/.claude/skills/reen-draft-format/SKILL.md`: add a short note under the App section that `web_app` requires a routing table and that routing is the entry, not a single entry context.
    - Run `reen init --upgrade` to sync the repo root.

11. **Tests.**
    - Workspace-emission tests: given a minimal prepared tree (one brand, one leaf component, one coordinating component, a two-route routing table, a `web_app` App entry), scaffold produces the four-crate layout. Each crate's `Cargo.toml` is well-formed and the inter-crate path dependencies resolve.
    - Component codegen tests: struct fields match props; `render` signature is correct; `on_<event>` methods exist per event; Composition dispatch helper matches the declared bindings.
    - RPC edge tests: a deliberately-planted server-only context invoked from a component emits both the server handler and the client stub with the expected path and payload shapes. A `client_also` context invoked from a component emits no RPC edge.
    - Routing-table codegen: `Router::dispatch` match covers every declared route; param decoders call the right manifest-resolved type.
    - Verify tests: each of the new verify cases has a "clean scaffold passes" and a "deliberately-broken scaffold fails with a targeted message" test.
    - WASM-classification sanity check: a test crate graph where a `client_also` context is made to depend on a `server_only` symbol — verify flags it.
    - Round-trip test: scaffold a fixture project, run `cargo check --workspace` inside it (in a tempdir), assert clean. This is a big test; guard it behind `#[ignore]` and a `--test-web-scaffold` cargo feature if it takes longer than 30 seconds. Don't skip it entirely — it is the only integration test that actually catches codegen bugs.
    - Regression: snake / money transfer / Connor prepared trees do not trigger web-app codegen (they have no `web_app` App entry). Their scaffold output is byte-identical to pre-Phase-10. Test this as a dedicated regression.

## Out of scope (strictly)

- Committing Reen to a framework (Leptos, Dioxus, Askama, Axum, etc.). The generated scaffold is intentionally framework-neutral; framework choice is an implementer decision made in `/implement`.
- A `screen` kind or a `store` kind. Both were explicitly rejected earlier in planning.
- CSR-only projects. The default is SSR; `--no-client` degrades to server-only. Pure-CSR (WASM + static HTML shell) is a future phase if anyone asks.
- A dev server, hot reload, or watch mode. `cargo watch` + `cargo run -p app-server` is the supported workflow.
- Changing the prepared-schema version. Phase 10 reads v3 as-is.
- Migrating any existing test project. Phase 11 owns that.
- New CLI subcommands. `reen scaffold` / `reen verify` get extended branches; the CLI surface doesn't change.
- Authentication / session handling. Operations carry their own auth via Phase 7's declarations; Reen does not invent a session layer.

## Commit pattern

- `phase 10: extend App kind with web_app variant`
- `phase 10: workspace layout emission for web_app`
- `phase 10: component codegen`
- `phase 10: brand codegen`
- `phase 10: routing-table codegen`
- `phase 10: WASM target inference and RPC edge emission`
- `phase 10: server binary and client WASM scaffolds`
- `phase 10: verify extensions for components / brand / routes / WASM classification`
- `phase 10: hooks and skills for web kinds`
- `phase 10: workspace-emission and round-trip tests`

If a defect in earlier phases surfaces while you work, fix it in its own `phase <n>: fix <defect>` commit before the dependent Phase 10 commit.

## Watchpoints

- **Framework neutrality is load-bearing.** The moment Reen emits a framework-specific macro (`view!`, `rsx!`, `Template`, etc.) in scaffold output, it is committed to that framework. Keep the emission to plain structs, plain functions, a `Markup` newtype, and `todo!()` bodies. Every framework can be adopted as a body-level choice without needing Reen to regenerate.
- **The `Markup` newtype is a contract, not a rendering system.** Define it once in `app-components::markup` as `pub struct Markup(pub String);` with a `Display` impl and an `html!(&'static str) -> Markup` constructor. Don't escape — escaping is the implementer's call. Don't add templating. If a future phase wants richer markup (streaming, async rendering), it can extend the newtype; for now, a String wrapper is enough to compile.
- **RPC edge path naming.** `/_rpc/<context-snake>/<functionality-snake>` is deterministic from the prepared YAML. Write down the exact algorithm in a comment at the emission site and assert on it in a test. If a BA renames a functionality, prepare → scaffold → verify will cleanly rename the path; if the rename gets lost along the way, that's a bug Phase 10 must not ship with.
- **Workspace member ordering.** Cargo workspaces care about member ordering in `Cargo.toml` for determinism in a subtle way. Sort members alphabetically in the emitted workspace `Cargo.toml`. Assert this in a test — otherwise dogfood idempotency will break on macOS/Linux filesystem ordering differences.
- **Path dependencies across the workspace.** Emit `app-core = { path = "../app-core" }` style path deps. Never use `version = "..."` — these crates aren't published. If someone later wants to publish pieces, that's a separate decision.
- **WASM-classification cycles.** Phase 9 was asked to detect cycles in the context→context dependency graph. If a cycle survives to Phase 10, the scaffolder will loop. Defensive: detect cycles here too and fail with an ambiguity naming the cycle. Shouldn't fire if Phase 9 did its job, but don't trust.
- **Serde derive duplication.** When adding `Serialize`/`Deserialize` to a data type for an RPC boundary, check whether the existing derive macro list already includes them. Appending duplicates is a compile error on some rustc versions — parse the existing derive attribute and union.
- **Server binary framework placeholder.** The "`todo!()` trait dispatched to the implementer's framework" approach works but is awkward. The alternative — a minimal `TcpListener` + thread-pool dispatch — is runnable out of the box and honest about what's missing. Prefer the runnable alternative. The implementer can refactor to axum/warp/tide in `/implement` without having to remove scaffolder-owned code first.
- **Static-asset path conventions.** CSS goes to `/static/brand.css`, WASM goes to `/static/app-client.wasm`. Write these paths into both the server's route table and the generated HTML shell. Don't hardcode them in three places with three spellings — derive from one constant in `app-components::paths`.
- **`@generated` marker discipline.** Phase 2.5's rule applies to every file Reen emits in Phase 10. Hand-edited files (marker removed) are preserved across scaffold runs. The marker text is the same canonical string Phase 2.5 introduced; do not invent a second variant.
- **Verify error wording stays DCI-language.** Every new verify case in Phase 10 must produce a message a BA could read. "The component `LoginForm` declares an event `submit` but the generated `on_submit` method is missing" — not "missing impl method: on_submit". A unit test per case locks the exact wording.
- **Cargo workspace root vs. crate-root Cargo.toml.** A workspace-root Cargo.toml that also has a `[package]` section is a Cargo anti-pattern and breaks some tooling. Keep the root strictly `[workspace]` with members. Phase 2.5's pattern of putting `[[bin]]` in the root Cargo.toml only applies to single-crate CLI/TUI projects — do not replicate it for web apps.
- **`--no-client` semantics.** When set, skip `app-client` entirely; emit component `on_<event>` handlers that use standard form-POST flows rather than WASM-bound listeners. The RPC edge emission still applies (server handler exists), but the client-side stub does not. Document this in the generated `main.rs`'s `@generated` comment.
- **Don't emit a database.** A routing table route whose component depends on a context that reads data does not imply a database. Reen's value types are in-memory; persistence is the implementer's decision. The scaffold must not create migrations, ORM code, or a connection pool. If the BA wants persistence, Phase 7's external-API pattern is the escape hatch (declare an API, consume it from operations).

## Finishing the phase

When you believe Phase 10 is done:

1. `cargo test` at the reen workspace root — green.
2. `cargo run -- init --upgrade` in the reen repo — `git status` clean (dogfood).
3. A minimal fixture web-app project scaffolds to a four-crate workspace where `cargo check --workspace` passes cleanly (with `todo!()` panics confined to method bodies, not type errors).
4. The same fixture, with a deliberate server-only context invoked from a component, produces the expected RPC edge shape on both sides (server handler + client stub). Verify passes.
5. The same fixture, with a deliberate `client_also` context invoked from a component, produces a direct-call edge — no RPC artefacts emitted. Verify passes.
6. A deliberately-broken scaffold (missing `on_submit`, missing `Router::dispatch` arm, or a `client_also` context depending on a `server_only` symbol) is rejected by verify with targeted DCI-language messages.
7. Snake / money transfer / Connor re-scaffold to byte-identical output compared to pre-Phase-10. The single-crate CLI/TUI path is untouched.
8. `reen scaffold --no-client` on the fixture produces a three-crate workspace (no `app-client`) that builds cleanly.
9. Update the task list: mark Phase 10 complete; unblock Phase 11 (web-app test project).
10. Report back with a one-paragraph summary, the generated `Cargo.toml` files from the fixture (workspace root + four members), the shape of one component's emitted `.rs`, and a brief note on the framework-neutrality tradeoff so Phase 11 can decide whether to commit to a specific framework for its test project.
