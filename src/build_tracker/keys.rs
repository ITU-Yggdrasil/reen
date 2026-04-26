//! Canonical artefact keys for the build tracker.
//!
//! One scheme used by both the tracker (population from prepared YAML) and
//! the scaffold (AST matching from generated Rust). Keys are derivable from
//! both directions.
//!
//! # Tracker entry kinds — state table
//!
//! Every entry tracked in `.reen/build.json` is keyed by an
//! [`ArtefactKey`] variant, classified by an [`ArtefactEntryKind`] tag,
//! and carries a state ([`ArtefactState`](super::ArtefactState):
//! `fresh` | `stale` | `not_implemented`). When the prepared YAML
//! changes, `reen prepare` recomputes a kind-specific *fragment hash*
//! from the prepared YAML; any hash change marks the entry stale and
//! cascades through the dependency graph (entries with this entry in
//! their `upstream` list). `reen scaffold` resets stale method bodies
//! to `todo!()` (and, for tracker-backed types, resets stale fields to
//! `()`). `/implement` flips stale to fresh by writing real content.
//! `/audit` can flip fresh back to stale with a finding attached.
//!
//! Each kind below is documented with five sub-headings:
//!
//! - **Path:** the canonical key string stored in `build.json`.
//! - **Tracks:** what concrete piece of generated source the entry
//!   corresponds to.
//! - **Fragment:** the prepared-YAML fields that go into the hash; any
//!   change here marks the entry stale.
//! - **State semantics:** how `fresh` / `stale` / `not_implemented`
//!   should be interpreted for this kind.
//! - **Consumed by:** which codegen / verify / audit paths read the
//!   entry.
//!
//! ## DCI kinds (data, context, projection)
//!
//! ### `Functionality { context, name }`
//!
//! - **Path:** `contexts/<context>/functionality/<name>`
//! - **Tracks:** the body of `pub fn <name>(&[mut] self, …)` in a
//!   context's emitted `impl` block.
//! - **Fragment:** context name, functionality name, prose
//!   description, and the `role_method_refs` slice (canonical role
//!   method names referenced by this functionality).
//! - **State semantics:** `fresh` once `/implement` writes a body that
//!   passes verify; `stale` after a draft edit cascades through;
//!   `not_implemented` on initial scaffold.
//! - **Consumed by:** `generate_context_surgical` (`src/codegen.rs`)
//!   to decide whether to preserve the existing body across
//!   re-scaffolds; `verify::flip_fresh_for_passing_methods`
//!   (`src/verify.rs`); `audit_persist::keys_for_finding`
//!   (`src/audit_persist.rs`).
//!
//! ### `RoleMethod { context, role, method }`
//!
//! - **Path:** `contexts/<context>/role/<role>/<method>`
//! - **Tracks:** the body of `fn <role>_<method>(&self, <role>: &T)`
//!   in a context's `impl` block.
//! - **Fragment:** context name, role, method name, canonical name
//!   (`<role>_<method>`), the role-method citation, and the optional
//!   `signature_hint`.
//! - **State semantics:** same lifecycle as `Functionality`.
//! - **Consumed by:** `generate_context_surgical`, `verify`,
//!   `audit_persist`, and `audit::collect_keys_for_finding`
//!   (`src/audit.rs`).
//!
//! ### `RoleType { context, role }`
//!
//! - **Path:** `contexts/<context>/role/<role>/type`
//! - **Tracks:** the role-player struct field declaration plus the
//!   role-type's class within the context (the type a role player is
//!   bound to).
//! - **Fragment:** context name, role, the data-type the role binds
//!   to, and the role's responsibility prose.
//! - **State semantics:** `fresh` once the field type is in place;
//!   stale when the role player is renamed, retyped, or its
//!   responsibility prose changes — cascades to every role method on
//!   that role.
//! - **Consumed by:** the cascade graph in
//!   `BuildTracker::cascade_stale` (every role method depends on its
//!   role type).
//!
//! ### `DataOperation { data, name }`
//!
//! - **Path:** `data/<data>/operation/<name>`
//! - **Tracks:** the body of `pub fn <name>(&self, …)` in a Data
//!   type's `impl` block.
//! - **Fragment:** data name, operation name, prose description.
//! - **State semantics:** `fresh` once `/implement` fills the body;
//!   `stale` after a description edit; `not_implemented` initially.
//! - **Consumed by:** `generate_data_surgical` (`src/codegen.rs`),
//!   `verify`, `audit_persist`.
//!
//! ### `DataConstructor { data }`
//!
//! - **Path:** `data/<data>/constructor`
//! - **Tracks:** the body of the mandatory
//!   `pub fn new(...) -> anyhow::Result<Self>` constructor.
//! - **Fragment:** data name, the literal `"new"`, the attribute
//!   names, and the invariant prose. Editing any attribute or
//!   invariant marks the constructor stale (the body's invariant
//!   enforcement may need to change).
//! - **State semantics:** `fresh` once the constructor enforces every
//!   invariant; `stale` after attribute or invariant edits.
//! - **Consumed by:** `generate_data_surgical`, `audit_persist`.
//!
//! ### `DataGetter { data, field }`
//!
//! - **Path:** `data/<data>/getter/<field>`
//! - **Tracks:** the deterministic
//!   `pub fn <field>(&self) -> &T` getter per declared attribute.
//! - **Fragment:** data name, attribute name, attribute description.
//! - **State semantics:** getters are emitted fully by the
//!   scaffolder, so `fresh` is the steady state; renaming or removing
//!   the attribute marks the getter stale and the next scaffold drops
//!   it. `/implement` rarely touches getter bodies.
//! - **Consumed by:** `generate_data_surgical`; `audit_persist`
//!   pairs getter and operation lookup.
//!
//! ### `DataType { data }`
//!
//! - **Path:** `data/<data>/type`
//! - **Tracks:** the `pub struct <Data> { … }` declaration itself.
//! - **Fragment:** data name, attribute-name list, invariant list.
//! - **State semantics:** stale on attribute add/remove/rename or
//!   invariant edit; cascades to every constructor, getter, operation,
//!   and per-attribute type entry on this data type.
//! - **Consumed by:** the cascade graph (every other Data-kind entry
//!   has this in its `upstream`); `audit_persist::keys_for_finding`.
//!
//! ### `DataAttributeType { data, attribute }`
//!
//! - **Path:** `data/<data>/attribute/<attribute>`
//! - **Tracks:** the Rust type chosen by `/implement` for the
//!   attribute's struct field. The actual type fragment lives in the
//!   scaffolded source; the entry only records whether the choice is
//!   still valid.
//! - **Fragment:** data name, attribute name, attribute description,
//!   multiplicity (`single` | `list`). A description edit or
//!   single↔list flip invalidates the chosen type.
//! - **State semantics:** `fresh` after `/implement` writes a real
//!   type and verify passes; `stale` reverts the field to `()` plus
//!   a TODO comment on the next scaffold; `not_implemented` is the
//!   initial state.
//! - **Consumed by:** `attribute_types_from_tracker_and_source`
//!   (`src/codegen.rs`) — preserves the previously-chosen type only
//!   when the entry is `fresh`.
//!
//! ### `DataVariantAttributeType { data, variant, attribute }`
//!
//! - **Path:** `data/<data>/variant/<variant>/attribute/<attribute>`
//! - **Tracks:** the Rust type chosen by `/implement` for one
//!   per-variant attribute inside `<Data>Payload`. Variant analogue
//!   of `DataAttributeType`.
//! - **Fragment:** data name, variant name, attribute name,
//!   description, multiplicity. Renaming the variant invalidates the
//!   type choice (the key changes), as does any description /
//!   multiplicity edit.
//! - **State semantics:** identical lifecycle to `DataAttributeType`.
//! - **Consumed by:**
//!   `variant_attribute_types_from_tracker_and_source`
//!   (`src/codegen.rs`).
//!
//! ### `ProjectionFunctionality { projection, name }`
//!
//! - **Path:** `projections/<projection>/functionality/<name>`
//! - **Tracks:** the body of `pub fn <name>(&self, …)` in a
//!   projection's `impl` block (queries are projection
//!   functionalities).
//! - **Fragment:** projection name, functionality name, description,
//!   `role_method_refs` slice.
//! - **State semantics:** same lifecycle as `Functionality`; verify
//!   additionally enforces `&self` (no `&mut self`).
//! - **Consumed by:** `generate_projection_surgical`, `verify`,
//!   `audit_persist`.
//!
//! ### `ProjectionRoleMethod { projection, role, method }`
//!
//! - **Path:** `projections/<projection>/role/<role>/<method>`
//! - **Tracks:** the body of a projection's private role method.
//! - **Fragment:** projection name, role, method name, canonical
//!   name, citation, optional signature hint.
//! - **State semantics:** same lifecycle as `RoleMethod`.
//! - **Consumed by:** `generate_projection_surgical`, `verify`,
//!   `audit_persist`.
//!
//! ### `ProjectionRoleType { projection, role }`
//!
//! - **Path:** `projections/<projection>/role/<role>/type`
//! - **Tracks:** the projection's role-player field declaration.
//! - **Fragment:** projection name, role, data type, responsibility.
//! - **State semantics:** same lifecycle as `RoleType`; cascades to
//!   every projection role method on that role.
//! - **Consumed by:** the cascade graph; the projection scaffold
//!   reads no body for this kind.
//!
//! ## Web-app kinds (Phase 10 multi-crate emission)
//!
//! Web kinds are seeded by `seed_web_tracker_entries`
//! (`src/web_codegen.rs`) at scaffold time with an empty
//! `prepared_hash` and `state: not_implemented`. They model code that
//! `scaffold_web_app` emits on top of the DCI kinds (router dispatch,
//! server entry, RPC stubs, brand CSS, component render bodies, …).
//! Today their fragment is degenerate (empty string) — they exist as
//! tracker placeholders so `/implement` and `/audit` can target them.
//! A future change might give them real fragments derived from
//! routing-table or component prose.
//!
//! ### `RouterArm { route }`
//!
//! - **Path:** `web/router/<route>`
//! - **Tracks:** one match arm in the generated `Router::dispatch`
//!   function (`<components-crate>/src/router.rs`). One entry per
//!   unique route component.
//! - **Fragment:** empty — placeholder.
//! - **State semantics:** initial scaffold seeds
//!   `not_implemented`; `/implement` is not expected to edit router
//!   bodies (they're deterministic from the routing table) but
//!   `/audit` can mark them stale with a finding.
//! - **Consumed by:** seeded by `seed_web_tracker_entries`
//!   (`src/web_codegen.rs:222`); not yet read by codegen body
//!   preservation.
//!
//! ### `ServerRoute { route }`
//!
//! - **Path:** `web/server/<route>`
//! - **Tracks:** one route handler in the generated server `main.rs`.
//!   The route name is the path with `/` substituted for `_`
//!   (`/accounts/{id}` → `_accounts_{id}`).
//! - **Fragment:** empty — placeholder.
//! - **State semantics:** identical to `RouterArm`.
//! - **Consumed by:** seeded by `seed_web_tracker_entries`.
//!
//! ### `RpcServer { context, functionality }`
//!
//! - **Path:** `web/rpc/server/<context>/<functionality>`
//! - **Tracks:** one server-side RPC stub for a component-event-to-
//!   server-context binding.
//! - **Fragment:** empty — placeholder.
//! - **State semantics:** identical to `RouterArm`.
//! - **Consumed by:** seeded by `seed_web_tracker_entries`.
//!
//! ### `RpcClient { context, functionality }`
//!
//! - **Path:** `web/rpc/client/<context>/<functionality>`
//! - **Tracks:** one client-side RPC stub matching `RpcServer` —
//!   only emitted when the project has a client crate.
//! - **Fragment:** empty — placeholder.
//! - **State semantics:** identical to `RouterArm`.
//! - **Consumed by:** seeded by `seed_web_tracker_entries`.
//!
//! ### `ClientInit { component }`
//!
//! - **Path:** `web/client/<component>`
//! - **Tracks:** one client-side init function per component that
//!   declares events.
//! - **Fragment:** empty — placeholder.
//! - **State semantics:** identical to `RouterArm`.
//! - **Consumed by:** seeded by `seed_web_tracker_entries`.
//!
//! ### `BrandCss { brand }`
//!
//! - **Path:** `web/brand/<brand>`
//! - **Tracks:** the generated CSS file for one brand artefact.
//! - **Fragment:** empty — placeholder.
//! - **State semantics:** identical to `RouterArm`.
//! - **Consumed by:** seeded by `seed_web_tracker_entries`.
//!
//! ### `ServerMain`
//!
//! - **Path:** `web/server/main`
//! - **Tracks:** the server crate's `main.rs` entry point. Singular
//!   (no per-instance fields).
//! - **Fragment:** empty — placeholder.
//! - **State semantics:** identical to `RouterArm`.
//! - **Consumed by:** seeded by `seed_web_tracker_entries`.
//!
//! ### `ComponentRender { component }`
//!
//! - **Path:** `web/component/<component>/render`
//! - **Tracks:** one component's `render` method body — the central
//!   unit of UI implementation work.
//! - **Fragment:** empty — placeholder.
//! - **State semantics:** identical to `RouterArm`; the
//!   `templates/.claude/commands/implement.md` contract section
//!   describes how `/implement` should fill render bodies.
//! - **Consumed by:** seeded by `seed_web_tracker_entries`. The
//!   tracker entry is read indirectly via the `audit_persist` →
//!   `extract_artefacts` flow, but body preservation for component
//!   render is owned by `write_generated_file`'s `@generated`-marker
//!   gate, not by tracker state.
//!
//! ### `SessionGuard { route }`
//!
//! - **Path:** `web/server/guard/<route>`
//! - **Tracks:** a placeholder for a per-route session guard
//!   middleware. Currently declared but never constructed by any
//!   codegen path — kept in the enum so the wire format absorbs
//!   future additions without a tracker schema bump.
//! - **Fragment:** N/A — no entries created.
//! - **State semantics:** N/A.
//! - **Consumed by:** parsed by the `FromStr` impl so historical
//!   `.reen/build.json` files survive a load if any user happened to
//!   write the key by hand. No production code constructs this kind.
//!
//! ## Lifecycle invariants (cross-kind)
//!
//! These four rules hold for every kind above:
//!
//! 1. **Initial state is `not_implemented`.** New entries (no
//!    matching key in the previous tracker) start `not_implemented`
//!    with `reason: InitialScaffold`.
//! 2. **Draft edits cascade.** When a draft change marks one entry
//!    stale (`reason: DraftChanged`), every entry with that key in
//!    its `upstream` is also marked stale (`reason:
//!    UpstreamChanged`). `cascade_stale` is BFS — each entry is
//!    reached at most once and keeps its first-seen reason.
//! 3. **`reen scaffold` resets stale, preserves fresh.** Stale
//!    method bodies become `todo!()` again; fresh ones survive byte-
//!    identically through the surgical re-emit. The tracker's state
//!    decides; the body lives in the source file.
//! 4. **`/audit` can flip fresh back to stale.** When the auditor
//!    finds a semantic-alignment violation it calls
//!    `mark_stale_audit(key, finding)` which sets `reason:
//!    AuditFinding` and attaches the finding text to the entry.
//!    `/implement` reads the finding to know what to repair.
//!
//! Examples of fully-formed key strings:
//!
//! - `contexts/money_transfer/functionality/transfer`
//! - `contexts/money_transfer/role/source/withdraw`
//! - `contexts/money_transfer/role/source/type`
//! - `data/account/operation/deposit`
//! - `data/account/type`
//! - `data/money/attribute/amount`
//! - `data/chat_turn/variant/tool_call/attribute/tool_input`
//! - `projections/balance/functionality/current_balance`

use std::fmt;
use std::str::FromStr;

/// The kind of trackable artefact within the build tracker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtefactEntryKind {
    Functionality,
    RoleMethod,
    DataOperation,
    DataConstructor,
    DataGetter,
    ProjectionFunctionality,
    ProjectionRoleMethod,
    ProjectionRoleType,
    RoleType,
    DataType,
    /// The Rust type chosen for a data attribute. Decided by `/implement`,
    /// preserved across re-scaffolds, marked stale when the attribute's
    /// description changes.
    DataAttributeType,
    DataVariantAttributeType,
    // Web-specific kinds (Phase 10)
    RouterArm,
    ServerRoute,
    RpcServer,
    RpcClient,
    ClientInit,
    BrandCss,
    ServerMain,
    ComponentRender,
    SessionGuard,
}

impl ArtefactEntryKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Functionality => "functionality",
            Self::RoleMethod => "role_method",
            Self::DataOperation => "data_operation",
            Self::DataConstructor => "data_constructor",
            Self::DataGetter => "data_getter",
            Self::ProjectionFunctionality => "projection_functionality",
            Self::ProjectionRoleMethod => "projection_role_method",
            Self::ProjectionRoleType => "projection_role_type",
            Self::RoleType => "role_type",
            Self::DataType => "data_type",
            Self::DataAttributeType => "data_attribute_type",
            Self::DataVariantAttributeType => "data_variant_attribute_type",
            Self::RouterArm => "router_arm",
            Self::ServerRoute => "server_route",
            Self::RpcServer => "rpc_server",
            Self::RpcClient => "rpc_client",
            Self::ClientInit => "client_init",
            Self::BrandCss => "brand_css",
            Self::ServerMain => "server_main",
            Self::ComponentRender => "component_render",
            Self::SessionGuard => "session_guard",
        }
    }
}

/// A canonical key identifying a single trackable artefact.
///
/// Display format matches the key strings stored in `.reen/build.json`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ArtefactKey {
    /// `contexts/<context>/functionality/<name>`
    Functionality {
        context: String,
        name: String,
    },
    /// `contexts/<context>/role/<role>/<method>`
    RoleMethod {
        context: String,
        role: String,
        method: String,
    },
    /// `contexts/<context>/role/<role>/type`
    RoleType {
        context: String,
        role: String,
    },
    /// `data/<data>/operation/<name>`
    DataOperation {
        data: String,
        name: String,
    },
    /// `data/<data>/constructor` — the mandatory `new()` constructor for a Data type.
    DataConstructor {
        data: String,
    },
    /// `data/<data>/getter/<field>` — one per declared attribute.
    DataGetter {
        data: String,
        field: String,
    },
    /// `data/<data>/type`
    DataType {
        data: String,
    },
    /// `data/<data>/attribute/<name>` — the Rust type chosen by `/implement`
    /// for one declared attribute. The actual type fragment lives in the
    /// scaffolded source; this entry only records whether the choice is
    /// `fresh` (use it), `stale` (description changed, re-implement), or
    /// `not_implemented` (initial state).
    DataAttributeType {
        data: String,
        attribute: String,
    },
    /// `data/<data>/variant/<variant>/attribute/<name>` — variant analogue
    /// of `DataAttributeType`. Variant attributes live inside the
    /// `<Name>Payload` enum and follow the same lifecycle (Fresh entries
    /// preserve `/implement`'s type; Stale/NotImplemented revert to `()`
    /// + TODO).
    DataVariantAttributeType {
        data: String,
        variant: String,
        attribute: String,
    },
    /// `projections/<projection>/functionality/<name>`
    ProjectionFunctionality {
        projection: String,
        name: String,
    },
    /// `projections/<projection>/role/<role>/<method>`
    ProjectionRoleMethod {
        projection: String,
        role: String,
        method: String,
    },
    /// `projections/<projection>/role/<role>/type`
    ProjectionRoleType {
        projection: String,
        role: String,
    },
    // ── Web-specific keys (Phase 10) ─────────────────────────
    /// `web/router/<route_name>`
    RouterArm { route: String },
    /// `web/server/<route_name>`
    ServerRoute { route: String },
    /// `web/rpc/server/<context>/<functionality>`
    RpcServer { context: String, functionality: String },
    /// `web/rpc/client/<context>/<functionality>`
    RpcClient { context: String, functionality: String },
    /// `web/client/<component>`
    ClientInit { component: String },
    /// `web/brand/<brand>`
    BrandCss { brand: String },
    /// `web/server/main`
    ServerMain,
    /// `web/component/<component>/render`
    ComponentRender { component: String },
    /// `web/server/guard/<route>`
    SessionGuard { route: String },
}

impl ArtefactKey {
    pub fn kind(&self) -> ArtefactEntryKind {
        match self {
            Self::Functionality { .. } => ArtefactEntryKind::Functionality,
            Self::RoleMethod { .. } => ArtefactEntryKind::RoleMethod,
            Self::RoleType { .. } => ArtefactEntryKind::RoleType,
            Self::DataOperation { .. } => ArtefactEntryKind::DataOperation,
            Self::DataConstructor { .. } => ArtefactEntryKind::DataConstructor,
            Self::DataGetter { .. } => ArtefactEntryKind::DataGetter,
            Self::DataType { .. } => ArtefactEntryKind::DataType,
            Self::DataAttributeType { .. } => ArtefactEntryKind::DataAttributeType,
            Self::DataVariantAttributeType { .. } => ArtefactEntryKind::DataVariantAttributeType,
            Self::ProjectionFunctionality { .. } => ArtefactEntryKind::ProjectionFunctionality,
            Self::ProjectionRoleMethod { .. } => ArtefactEntryKind::ProjectionRoleMethod,
            Self::ProjectionRoleType { .. } => ArtefactEntryKind::ProjectionRoleType,
            Self::RouterArm { .. } => ArtefactEntryKind::RouterArm,
            Self::ServerRoute { .. } => ArtefactEntryKind::ServerRoute,
            Self::RpcServer { .. } => ArtefactEntryKind::RpcServer,
            Self::RpcClient { .. } => ArtefactEntryKind::RpcClient,
            Self::ClientInit { .. } => ArtefactEntryKind::ClientInit,
            Self::BrandCss { .. } => ArtefactEntryKind::BrandCss,
            Self::ServerMain => ArtefactEntryKind::ServerMain,
            Self::ComponentRender { .. } => ArtefactEntryKind::ComponentRender,
            Self::SessionGuard { .. } => ArtefactEntryKind::SessionGuard,
        }
    }

    // ── Constructors from prepared YAML ──────────────────────────

    pub fn functionality(context: &str, name: &str) -> Self {
        Self::Functionality {
            context: crate::codegen::to_snake_case(context),
            name: crate::codegen::to_snake_case(name),
        }
    }

    pub fn role_method(context: &str, role: &str, method: &str) -> Self {
        Self::RoleMethod {
            context: crate::codegen::to_snake_case(context),
            role: crate::codegen::to_snake_case(role),
            method: crate::codegen::to_snake_case(method),
        }
    }

    pub fn role_type(context: &str, role: &str) -> Self {
        Self::RoleType {
            context: crate::codegen::to_snake_case(context),
            role: crate::codegen::to_snake_case(role),
        }
    }

    pub fn data_operation(data: &str, name: &str) -> Self {
        Self::DataOperation {
            data: crate::codegen::to_snake_case(data),
            name: crate::codegen::to_snake_case(name),
        }
    }

    pub fn data_constructor(data: &str) -> Self {
        Self::DataConstructor {
            data: crate::codegen::to_snake_case(data),
        }
    }

    pub fn data_getter(data: &str, field: &str) -> Self {
        Self::DataGetter {
            data: crate::codegen::to_snake_case(data),
            field: crate::codegen::to_snake_case(field),
        }
    }

    pub fn data_type(data: &str) -> Self {
        Self::DataType {
            data: crate::codegen::to_snake_case(data),
        }
    }

    pub fn data_attribute_type(data: &str, attribute: &str) -> Self {
        Self::DataAttributeType {
            data: crate::codegen::to_snake_case(data),
            attribute: crate::codegen::to_snake_case(attribute),
        }
    }

    pub fn data_variant_attribute_type(data: &str, variant: &str, attribute: &str) -> Self {
        Self::DataVariantAttributeType {
            data: crate::codegen::to_snake_case(data),
            variant: crate::codegen::to_snake_case(variant),
            attribute: crate::codegen::to_snake_case(attribute),
        }
    }

    pub fn projection_functionality(projection: &str, name: &str) -> Self {
        Self::ProjectionFunctionality {
            projection: crate::codegen::to_snake_case(projection),
            name: crate::codegen::to_snake_case(name),
        }
    }

    pub fn projection_role_method(projection: &str, role: &str, method: &str) -> Self {
        Self::ProjectionRoleMethod {
            projection: crate::codegen::to_snake_case(projection),
            role: crate::codegen::to_snake_case(role),
            method: crate::codegen::to_snake_case(method),
        }
    }

    pub fn projection_role_type(projection: &str, role: &str) -> Self {
        Self::ProjectionRoleType {
            projection: crate::codegen::to_snake_case(projection),
            role: crate::codegen::to_snake_case(role),
        }
    }

    // ── Constructors from Rust AST ───────────────────────────────

    /// Derive a key from a Rust method in an impl block, given the file path
    /// and enclosing type name.
    ///
    /// `file_subdir` is one of "contexts", "data", "projections".
    /// `type_name` is the snake_case filename stem (e.g. "money_transfer").
    /// `method_name` is the method identifier.
    /// `is_public` indicates whether the method is `pub`.
    pub fn from_rust_method(
        file_subdir: &str,
        type_name: &str,
        method_name: &str,
        is_public: bool,
    ) -> Option<Self> {
        let type_snake = crate::codegen::to_snake_case(type_name);
        let method_snake = crate::codegen::to_snake_case(method_name);

        match file_subdir {
            "contexts" => {
                if is_public {
                    Some(Self::Functionality {
                        context: type_snake,
                        name: method_snake,
                    })
                } else {
                    // Private method — try to parse as role_method: <role>_<method>
                    // We can't know the role from just the method name without the
                    // prepared YAML, so we store the canonical name directly.
                    // The caller must resolve this against the prepared artifact.
                    None
                }
            }
            "data" => {
                if is_public {
                    Some(Self::DataOperation {
                        data: type_snake,
                        name: method_snake,
                    })
                } else {
                    None
                }
            }
            "projections" => {
                if is_public {
                    Some(Self::ProjectionFunctionality {
                        projection: type_snake,
                        name: method_snake,
                    })
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Derive a key for a private context method using the known role method
    /// canonical name from the prepared YAML.
    pub fn from_rust_role_method(
        context: &str,
        role: &str,
        method: &str,
    ) -> Self {
        Self::RoleMethod {
            context: crate::codegen::to_snake_case(context),
            role: crate::codegen::to_snake_case(role),
            method: crate::codegen::to_snake_case(method),
        }
    }
}

impl fmt::Display for ArtefactKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Functionality { context, name } => {
                write!(f, "contexts/{}/functionality/{}", context, name)
            }
            Self::RoleMethod { context, role, method } => {
                write!(f, "contexts/{}/role/{}/{}", context, role, method)
            }
            Self::RoleType { context, role } => {
                write!(f, "contexts/{}/role/{}/type", context, role)
            }
            Self::DataOperation { data, name } => {
                write!(f, "data/{}/operation/{}", data, name)
            }
            Self::DataConstructor { data } => {
                write!(f, "data/{}/constructor", data)
            }
            Self::DataGetter { data, field } => {
                write!(f, "data/{}/getter/{}", data, field)
            }
            Self::DataType { data } => {
                write!(f, "data/{}/type", data)
            }
            Self::DataAttributeType { data, attribute } => {
                write!(f, "data/{}/attribute/{}", data, attribute)
            }
            Self::DataVariantAttributeType { data, variant, attribute } => {
                write!(f, "data/{}/variant/{}/attribute/{}", data, variant, attribute)
            }
            Self::ProjectionFunctionality { projection, name } => {
                write!(f, "projections/{}/functionality/{}", projection, name)
            }
            Self::ProjectionRoleMethod { projection, role, method } => {
                write!(f, "projections/{}/role/{}/{}", projection, role, method)
            }
            Self::ProjectionRoleType { projection, role } => {
                write!(f, "projections/{}/role/{}/type", projection, role)
            }
            Self::RouterArm { route } => write!(f, "web/router/{}", route),
            Self::ServerRoute { route } => write!(f, "web/server/{}", route),
            Self::RpcServer { context, functionality } => {
                write!(f, "web/rpc/server/{}/{}", context, functionality)
            }
            Self::RpcClient { context, functionality } => {
                write!(f, "web/rpc/client/{}/{}", context, functionality)
            }
            Self::ClientInit { component } => write!(f, "web/client/{}", component),
            Self::BrandCss { brand } => write!(f, "web/brand/{}", brand),
            Self::ServerMain => write!(f, "web/server/main"),
            Self::ComponentRender { component } => write!(f, "web/component/{}/render", component),
            Self::SessionGuard { route } => write!(f, "web/server/guard/{}", route),
        }
    }
}

impl FromStr for ArtefactKey {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.split('/').collect();
        match parts.as_slice() {
            ["contexts", ctx, "functionality", name] => Ok(Self::Functionality {
                context: ctx.to_string(),
                name: name.to_string(),
            }),
            ["contexts", ctx, "role", role, "type"] => Ok(Self::RoleType {
                context: ctx.to_string(),
                role: role.to_string(),
            }),
            ["contexts", ctx, "role", role, method] => Ok(Self::RoleMethod {
                context: ctx.to_string(),
                role: role.to_string(),
                method: method.to_string(),
            }),
            ["data", data, "operation", name] => Ok(Self::DataOperation {
                data: data.to_string(),
                name: name.to_string(),
            }),
            ["data", data, "constructor"] => Ok(Self::DataConstructor {
                data: data.to_string(),
            }),
            ["data", data, "getter", field] => Ok(Self::DataGetter {
                data: data.to_string(),
                field: field.to_string(),
            }),
            ["data", data, "type"] => Ok(Self::DataType {
                data: data.to_string(),
            }),
            ["data", data, "attribute", attribute] => Ok(Self::DataAttributeType {
                data: data.to_string(),
                attribute: attribute.to_string(),
            }),
            ["data", data, "variant", variant, "attribute", attribute] => {
                Ok(Self::DataVariantAttributeType {
                    data: data.to_string(),
                    variant: variant.to_string(),
                    attribute: attribute.to_string(),
                })
            }
            ["projections", proj, "functionality", name] => Ok(Self::ProjectionFunctionality {
                projection: proj.to_string(),
                name: name.to_string(),
            }),
            ["projections", proj, "role", role, "type"] => Ok(Self::ProjectionRoleType {
                projection: proj.to_string(),
                role: role.to_string(),
            }),
            ["projections", proj, "role", role, method] => Ok(Self::ProjectionRoleMethod {
                projection: proj.to_string(),
                role: role.to_string(),
                method: method.to_string(),
            }),
            // Web-specific keys
            ["web", "router", route] => Ok(Self::RouterArm { route: route.to_string() }),
            ["web", "server", "main"] => Ok(Self::ServerMain),
            ["web", "server", route] => Ok(Self::ServerRoute { route: route.to_string() }),
            ["web", "rpc", "server", ctx, func] => Ok(Self::RpcServer {
                context: ctx.to_string(),
                functionality: func.to_string(),
            }),
            ["web", "rpc", "client", ctx, func] => Ok(Self::RpcClient {
                context: ctx.to_string(),
                functionality: func.to_string(),
            }),
            ["web", "client", comp] => Ok(Self::ClientInit { component: comp.to_string() }),
            ["web", "brand", brand] => Ok(Self::BrandCss { brand: brand.to_string() }),
            ["web", "component", comp, "render"] => Ok(Self::ComponentRender { component: comp.to_string() }),
            ["web", "server", "guard", route] => Ok(Self::SessionGuard { route: route.to_string() }),
            _ => Err(format!("invalid artefact key: {}", s)),
        }
    }
}

// Serde: serialize as the display string
impl serde::Serialize for ArtefactKey {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> serde::Deserialize<'de> for ArtefactKey {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_and_parse_round_trip() {
        let keys = vec![
            ArtefactKey::functionality("MoneyTransfer", "transfer"),
            ArtefactKey::role_method("MoneyTransfer", "source", "withdraw"),
            ArtefactKey::role_type("MoneyTransfer", "source"),
            ArtefactKey::data_operation("Account", "deposit"),
            ArtefactKey::data_constructor("Account"),
            ArtefactKey::data_getter("Account", "balance"),
            ArtefactKey::data_type("Account"),
            ArtefactKey::data_attribute_type("Account", "balance"),
            ArtefactKey::projection_functionality("AccountBalance", "current_balance"),
        ];

        for key in &keys {
            let s = key.to_string();
            let parsed: ArtefactKey = s.parse().unwrap();
            assert_eq!(&parsed, key, "round-trip failed for {}", s);
        }
    }

    #[test]
    fn data_attribute_type_key_format() {
        // Bug 2.5c regression: the key shape is `data/<snake>/attribute/<snake>`.
        let key = ArtefactKey::data_attribute_type("Money", "amount");
        assert_eq!(key.to_string(), "data/money/attribute/amount");
        assert_eq!(key.kind(), ArtefactEntryKind::DataAttributeType);

        let parsed: ArtefactKey = "data/money/attribute/amount".parse().unwrap();
        assert_eq!(parsed, key);
    }

    #[test]
    fn data_variant_attribute_type_key_format() {
        // Group 4 regression: the key shape is
        // `data/<snake>/variant/<snake>/attribute/<snake>`.
        let key = ArtefactKey::data_variant_attribute_type(
            "ChatTurn",
            "tool_call",
            "tool_input",
        );
        assert_eq!(key.to_string(), "data/chat_turn/variant/tool_call/attribute/tool_input");
        assert_eq!(key.kind(), ArtefactEntryKind::DataVariantAttributeType);

        let parsed: ArtefactKey =
            "data/chat_turn/variant/tool_call/attribute/tool_input".parse().unwrap();
        assert_eq!(parsed, key);
    }

    #[test]
    fn data_constructor_and_getter_display_format() {
        assert_eq!(
            ArtefactKey::data_constructor("Money").to_string(),
            "data/money/constructor"
        );
        assert_eq!(
            ArtefactKey::data_getter("Money", "currency").to_string(),
            "data/money/getter/currency"
        );
    }

    #[test]
    fn data_constructor_and_getter_kind_classification() {
        assert_eq!(
            ArtefactKey::data_constructor("Money").kind(),
            ArtefactEntryKind::DataConstructor
        );
        assert_eq!(
            ArtefactKey::data_getter("Money", "currency").kind(),
            ArtefactEntryKind::DataGetter
        );
    }

    #[test]
    fn display_format() {
        assert_eq!(
            ArtefactKey::functionality("MoneyTransfer", "transfer").to_string(),
            "contexts/money_transfer/functionality/transfer"
        );
        assert_eq!(
            ArtefactKey::role_method("MoneyTransfer", "source", "withdraw").to_string(),
            "contexts/money_transfer/role/source/withdraw"
        );
        assert_eq!(
            ArtefactKey::role_type("MoneyTransfer", "source").to_string(),
            "contexts/money_transfer/role/source/type"
        );
        assert_eq!(
            ArtefactKey::data_operation("Account", "deposit").to_string(),
            "data/account/operation/deposit"
        );
        assert_eq!(
            ArtefactKey::data_type("Account").to_string(),
            "data/account/type"
        );
        assert_eq!(
            ArtefactKey::projection_functionality("AccountBalance", "current_balance").to_string(),
            "projections/account_balance/functionality/current_balance"
        );
    }

    #[test]
    fn parse_invalid_key() {
        assert!("invalid/key".parse::<ArtefactKey>().is_err());
        assert!("".parse::<ArtefactKey>().is_err());
        assert!("contexts/foo".parse::<ArtefactKey>().is_err());
    }

    #[test]
    fn serde_json_round_trip() {
        let key = ArtefactKey::functionality("MoneyTransfer", "transfer");
        let json = serde_json::to_string(&key).unwrap();
        assert_eq!(json, "\"contexts/money_transfer/functionality/transfer\"");
        let parsed: ArtefactKey = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, key);
    }

    #[test]
    fn kind_classification() {
        assert_eq!(
            ArtefactKey::functionality("X", "y").kind(),
            ArtefactEntryKind::Functionality
        );
        assert_eq!(
            ArtefactKey::role_method("X", "r", "m").kind(),
            ArtefactEntryKind::RoleMethod
        );
        assert_eq!(
            ArtefactKey::role_type("X", "r").kind(),
            ArtefactEntryKind::RoleType
        );
        assert_eq!(
            ArtefactKey::data_operation("X", "y").kind(),
            ArtefactEntryKind::DataOperation
        );
        assert_eq!(
            ArtefactKey::data_type("X").kind(),
            ArtefactEntryKind::DataType
        );
        assert_eq!(
            ArtefactKey::projection_functionality("X", "y").kind(),
            ArtefactEntryKind::ProjectionFunctionality
        );
        assert_eq!(
            ArtefactKey::projection_role_type("X", "r").kind(),
            ArtefactEntryKind::ProjectionRoleType
        );
        assert_eq!(
            ArtefactKey::projection_role_method("X", "r", "m").kind(),
            ArtefactEntryKind::ProjectionRoleMethod
        );
    }

    #[test]
    fn from_rust_method_contexts() {
        let key = ArtefactKey::from_rust_method("contexts", "money_transfer", "transfer", true);
        assert_eq!(key, Some(ArtefactKey::functionality("money_transfer", "transfer")));

        // Private methods in contexts can't be resolved without prepared YAML
        let key = ArtefactKey::from_rust_method("contexts", "money_transfer", "source_withdraw", false);
        assert_eq!(key, None);
    }

    #[test]
    fn from_rust_method_data() {
        let key = ArtefactKey::from_rust_method("data", "account", "deposit", true);
        assert_eq!(key, Some(ArtefactKey::data_operation("account", "deposit")));

        let key = ArtefactKey::from_rust_method("data", "account", "helper", false);
        assert_eq!(key, None);
    }

    #[test]
    fn from_rust_method_projections() {
        let key = ArtefactKey::from_rust_method("projections", "account_balance", "current_balance", true);
        assert_eq!(key, Some(ArtefactKey::projection_functionality("account_balance", "current_balance")));
    }
}
