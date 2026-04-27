//! Structural alignment verification between Rust source and prepared v2 YAML.
//!
//! Parses a `.rs` file with `syn` and compares its structure against the
//! prepared artifact. Emits a structured diff in DCI vocabulary on mismatch.

use crate::prepared::{ArtifactKind, PreparedArtifact};
use anyhow::{Context, Result};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

/// A single discrepancy between the Rust source and the prepared artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discrepancy {
    pub kind: DiscrepancyKind,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscrepancyKind {
    MissingField,
    ExtraField,
    MissingMethod,
    ExtraMethod,
    WrongVisibility,
}

/// Result of verification.
#[derive(Debug, Clone)]
pub struct VerifyResult {
    pub discrepancies: Vec<Discrepancy>,
}

impl VerifyResult {
    pub fn is_ok(&self) -> bool {
        self.discrepancies.is_empty()
    }

    pub fn render(&self) -> String {
        if self.is_ok() {
            return "✓ Rust source matches prepared specification.".to_string();
        }
        let mut out = String::new();
        for d in &self.discrepancies {
            out.push_str(&format!("- {:?}: {}\n", d.kind, d.message));
        }
        out
    }
}

/// Verify a Rust source file against a prepared artifact.
///
/// Runs the structural checks only. To additionally enforce the kind-discipline
/// rules (private fields, mandatory constructors, getter presence, immutability,
/// field-type containment), call `verify_file_with_kind`.
pub fn verify_file(rs_path: &Path, artifact: &PreparedArtifact) -> Result<VerifyResult> {
    let source = fs::read_to_string(rs_path)
        .with_context(|| format!("Failed to read {}", rs_path.display()))?;
    verify_source(&source, artifact)
}

/// Verify a Rust source file against a prepared artifact, including
/// kind-discipline structural gates.
///
/// `kind_index` resolves type names to artefact kinds for field-type
/// containment checks (Data may reference Data only; Projection may reference
/// Data or Projection).
pub fn verify_file_with_kind(
    rs_path: &Path,
    artifact: &PreparedArtifact,
    kind_index: &crate::verify_kind::KindIndex,
) -> Result<VerifyResult> {
    let source = fs::read_to_string(rs_path)
        .with_context(|| format!("Failed to read {}", rs_path.display()))?;
    let mut result = verify_source(&source, artifact)?;
    let file: syn::File = syn::parse_str(&source)
        .map_err(|e| anyhow::anyhow!("Failed to parse Rust source: {}", e))?;
    let kind_findings = crate::verify_kind::check(&file, artifact, kind_index);
    result.discrepancies.extend(kind_findings);
    let drift_findings = crate::verify_drift::check(&file, artifact);
    result.discrepancies.extend(drift_findings);
    Ok(result)
}

/// Verify Rust source string against a prepared artifact.
pub fn verify_source(source: &str, artifact: &PreparedArtifact) -> Result<VerifyResult> {
    let file: syn::File = syn::parse_str(source)
        .map_err(|e| anyhow::anyhow!("Failed to parse Rust source: {}", e))?;

    let export_name = &artifact.export.name;
    let mut discrepancies = Vec::new();

    // Find the struct or enum
    let struct_item = file.items.iter().find_map(|item| {
        if let syn::Item::Struct(s) = item {
            if s.ident == export_name { Some(s) } else { None }
        } else {
            None
        }
    });

    let enum_item = file.items.iter().find_map(|item| {
        if let syn::Item::Enum(e) = item {
            if e.ident == export_name { Some(e) } else { None }
        } else {
            None
        }
    });

    if struct_item.is_none() && enum_item.is_none() {
        discrepancies.push(Discrepancy {
            kind: DiscrepancyKind::MissingField,
            message: format!("struct `{}` not found in source", export_name),
        });
        return Ok(VerifyResult { discrepancies });
    }

    // Collect actual fields (from struct fields or enum variants)
    let actual_fields: BTreeSet<String> = if let Some(s) = struct_item {
        s.fields
            .iter()
            .filter_map(|f| f.ident.as_ref().map(|i| i.to_string()))
            .collect()
    } else if let Some(e) = enum_item {
        e.variants
            .iter()
            .map(|v| crate::codegen::to_snake_case(&v.ident.to_string()))
            .collect()
    } else {
        BTreeSet::new()
    };

    // Collect expected fields from artifact
    let expected_fields = expected_fields(artifact);

    // Check fields
    for name in expected_fields.difference(&actual_fields) {
        let label = field_label(artifact, name);
        discrepancies.push(Discrepancy {
            kind: DiscrepancyKind::MissingField,
            message: format!("{} `{}` is missing from the struct", label, name),
        });
    }
    // Extra fields are allowed on all artifact kinds:
    // - Contexts need internal bookkeeping (event buffers, caches, counters)
    //   beyond the DCI-specified role players and props.
    // - Projections need internal state beyond `source`.
    // - Data types with no YAML attributes may carry their own fields.
    // The verifier checks ⊇ (all expected fields present), not = (exact match).
    // This mirrors the private-method ⊇ treatment already in place.
    let check_extra = match artifact.kind {
        ArtifactKind::Context => false,
        ArtifactKind::Data => !expected_fields.is_empty(),
        ArtifactKind::Projection => false,
        // Phase 10 will extend verify for web kinds
        ArtifactKind::Component => false, // Components may carry internal bookkeeping
        ArtifactKind::Brand | ArtifactKind::RoutingTable => false,
        ArtifactKind::App => false, // App verification handled separately
    };
    if check_extra {
        for name in actual_fields.difference(&expected_fields) {
            discrepancies.push(Discrepancy {
                kind: DiscrepancyKind::ExtraField,
                message: format!("field `{}` is in the struct but not in the specification", name),
            });
        }
    }

    // Find impl blocks for the export type
    let impl_blocks: Vec<&syn::ItemImpl> = file
        .items
        .iter()
        .filter_map(|item| {
            if let syn::Item::Impl(imp) = item {
                if imp.trait_.is_none() {
                    if let syn::Type::Path(tp) = imp.self_ty.as_ref() {
                        if tp.path.is_ident(export_name) {
                            return Some(imp);
                        }
                    }
                }
            }
            None
        })
        .collect();

    // Collect all methods from all impl blocks
    let mut actual_pub_methods = BTreeSet::new();
    let mut actual_priv_methods = BTreeSet::new();
    for imp in &impl_blocks {
        for item in &imp.items {
            if let syn::ImplItem::Fn(method) = item {
                let name = method.sig.ident.to_string();
                if matches!(method.vis, syn::Visibility::Public(_)) {
                    actual_pub_methods.insert(name);
                } else {
                    actual_priv_methods.insert(name);
                }
            }
        }
    }

    // Expected methods
    let (expected_pub, expected_priv) = expected_methods(artifact);

    // Check public methods
    for name in expected_pub.difference(&actual_pub_methods) {
        let label = pub_method_label(artifact, name);
        discrepancies.push(Discrepancy {
            kind: DiscrepancyKind::MissingMethod,
            message: format!("{} `{}` is missing from the implementation", label, name),
        });
    }
    // A public method that should be private
    for name in &expected_priv {
        if actual_pub_methods.contains(name) && !expected_pub.contains(name) {
            discrepancies.push(Discrepancy {
                kind: DiscrepancyKind::WrongVisibility,
                message: format!(
                    "role method `{}` should be private but is declared `pub`",
                    name
                ),
            });
        }
    }

    // Check private methods (⊇ not =, per plan: "private method set ⊇ role_methods")
    for name in expected_priv.difference(&actual_priv_methods) {
        // It might be in pub methods (wrong visibility, already reported above)
        if !actual_pub_methods.contains(name) {
            discrepancies.push(Discrepancy {
                kind: DiscrepancyKind::MissingMethod,
                message: format!("role method `{}` is missing from the implementation", name),
            });
        }
    }

    Ok(VerifyResult { discrepancies })
}

// ── Expected shape extraction ─────────────────────────────────────

fn expected_fields(artifact: &PreparedArtifact) -> BTreeSet<String> {
    let mut fields = BTreeSet::new();
    match artifact.kind {
        ArtifactKind::Context => {
            for rp in &artifact.role_players {
                fields.insert(crate::codegen::to_snake_case(&rp.name));
            }
            for prop in &artifact.props {
                fields.insert(crate::codegen::to_snake_case(&prop.name));
            }
        }
        ArtifactKind::Data => {
            for attr in &artifact.attributes {
                fields.insert(crate::codegen::to_snake_case(&attr.name));
            }
        }
        ArtifactKind::Projection => {
            // Projections share the context shape: role players + props become
            // private struct fields.
            for rp in &artifact.role_players {
                fields.insert(crate::codegen::to_snake_case(&rp.name));
            }
            for prop in &artifact.props {
                fields.insert(crate::codegen::to_snake_case(&prop.name));
            }
        }
        ArtifactKind::Component => {
            for prop in &artifact.props {
                fields.insert(crate::codegen::to_snake_case(&prop.name));
            }
            if !artifact.variants.is_empty() {
                fields.insert("variant".to_string());
            }
        }
        ArtifactKind::Brand | ArtifactKind::RoutingTable => {}
        ArtifactKind::App => {} // App fields verified separately
    }
    fields
}

/// Returns (expected_public, expected_private) method name sets.
fn expected_methods(artifact: &PreparedArtifact) -> (BTreeSet<String>, BTreeSet<String>) {
    let mut pub_methods = BTreeSet::new();
    let mut priv_methods = BTreeSet::new();

    match artifact.kind {
        ArtifactKind::Context => {
            for func in &artifact.functionalities {
                pub_methods.insert(crate::codegen::to_snake_case(&func.name));
            }
            for group in &artifact.role_methods {
                for rm in &group.methods {
                    priv_methods.insert(rm.canonical_name.clone());
                }
            }
        }
        ArtifactKind::Data => {
            for op in &artifact.operations {
                pub_methods.insert(crate::codegen::to_snake_case(&op.name));
            }
        }
        ArtifactKind::Projection => {
            // Projection public surface = declared functionalities + the
            // mandatory `new` constructor. Role methods are private.
            pub_methods.insert("new".to_string());
            for func in &artifact.functionalities {
                pub_methods.insert(crate::codegen::to_snake_case(&func.name));
            }
            for group in &artifact.role_methods {
                for rm in &group.methods {
                    priv_methods.insert(rm.canonical_name.clone());
                }
            }
        }
        ArtifactKind::Component => {
            pub_methods.insert("render".to_string());
            for event in &artifact.events {
                pub_methods.insert(format!("on_{}", crate::codegen::to_snake_case(&event.name)));
            }
        }
        ArtifactKind::RoutingTable => {
            pub_methods.insert("dispatch".to_string());
            pub_methods.insert("render".to_string());
        }
        ArtifactKind::Brand | ArtifactKind::App => {}
    }

    (pub_methods, priv_methods)
}

fn field_label(artifact: &PreparedArtifact, name: &str) -> &'static str {
    match artifact.kind {
        ArtifactKind::Context => {
            if artifact.role_players.iter().any(|rp| crate::codegen::to_snake_case(&rp.name) == name) {
                "role player"
            } else {
                "prop"
            }
        }
        ArtifactKind::Data => "attribute",
        ArtifactKind::Projection => "source field",
        ArtifactKind::Component => {
            if artifact.props.iter().any(|p| crate::codegen::to_snake_case(&p.name) == name) {
                "prop"
            } else {
                "field"
            }
        }
        ArtifactKind::Brand | ArtifactKind::RoutingTable | ArtifactKind::App => "field",
    }
}

fn pub_method_label(artifact: &PreparedArtifact, name: &str) -> &'static str {
    match artifact.kind {
        ArtifactKind::Context => "functionality",
        ArtifactKind::Data => "operation",
        ArtifactKind::Projection => "query",
        ArtifactKind::Component => {
            if name == "render" {
                "render method"
            } else if name.starts_with("on_") {
                "event handler"
            } else {
                "method"
            }
        }
        ArtifactKind::RoutingTable => {
            if name == "dispatch" || name == "render" {
                "router method"
            } else {
                "method"
            }
        }
        ArtifactKind::Brand | ArtifactKind::App => "method",
    }
}

// ── App verification ─────────────────────────────────────────────

/// Verify that a project's `src/main.rs` and `Cargo.toml` match the App artifact.
///
/// Checks:
/// - `src/main.rs` exists
/// - It contains a `fn main` that references the declared entry context by name
/// - It calls the declared entry functionality on that context
/// - `Cargo.toml` has a `[[bin]]` entry whose name matches the package name
pub fn verify_app(workspace_root: &Path, artifact: &PreparedArtifact) -> VerifyResult {
    let mut discrepancies = Vec::new();

    let entry_context = artifact.entry_context.as_deref().unwrap_or("App");
    let entry_functionality = artifact.entry_functionality.as_deref().unwrap_or("run");

    // Check src/main.rs exists
    let main_path = workspace_root.join("src/main.rs");
    if !main_path.is_file() {
        discrepancies.push(Discrepancy {
            kind: DiscrepancyKind::MissingField,
            message: format!(
                "the app declares `{}` as the entry context but `src/main.rs` does not exist — \
                 run `reen scaffold` to generate it",
                entry_context
            ),
        });
        return VerifyResult { discrepancies };
    }

    // Read and check main.rs content
    if let Ok(content) = fs::read_to_string(&main_path) {
        if !content.contains(entry_context) {
            discrepancies.push(Discrepancy {
                kind: DiscrepancyKind::MissingField,
                message: format!(
                    "the app declares `{}` as the entry context but `src/main.rs` does not reference it",
                    entry_context
                ),
            });
        }
        let func_snake = crate::codegen::to_snake_case(entry_functionality);
        if !content.contains(&func_snake) {
            discrepancies.push(Discrepancy {
                kind: DiscrepancyKind::MissingMethod,
                message: format!(
                    "the app declares `{}` as the entry functionality but `src/main.rs` does not call it",
                    entry_functionality
                ),
            });
        }
    }

    // Check Cargo.toml for [[bin]] entry
    let cargo_path = workspace_root.join("Cargo.toml");
    if cargo_path.is_file() {
        if let Ok(raw) = fs::read_to_string(&cargo_path) {
            if let Ok(doc) = raw.parse::<toml_edit::DocumentMut>() {
                let package_name = doc
                    .get("package")
                    .and_then(|p| p.get("name"))
                    .and_then(|n| n.as_str())
                    .unwrap_or("");

                let has_bin = doc.get("bin").map_or(false, |bin| {
                    bin.as_array_of_tables().map_or(false, |tables| {
                        tables.iter().any(|t| {
                            t.get("name").and_then(|n| n.as_str()) == Some(package_name)
                        })
                    })
                });

                if !has_bin {
                    discrepancies.push(Discrepancy {
                        kind: DiscrepancyKind::MissingField,
                        message: format!(
                            "the app declares `{}` as the binary entry but Cargo.toml has no `[[bin]]` section \
                             with `name = \"{}\"`",
                            package_name, package_name
                        ),
                    });
                }
            }
        }
    }

    VerifyResult { discrepancies }
}

// ── Tracker integration ──────────────────────────────────────────

/// After a successful verify, flip any stale tracker entries for this
/// artifact to `fresh`. Called by the CLI after verify passes.
pub fn flip_verified_to_fresh(
    rs_path: &Path,
    artifact: &PreparedArtifact,
    _workspace_root: &Path,
    state_dir: &Path,
) -> Result<()> {
    use crate::build_tracker::{ArtefactState, BuildTracker};

    let mut tracker = BuildTracker::load(state_dir)?;
    if tracker.artefacts.is_empty() {
        return Ok(()); // No tracker, nothing to flip
    }

    let export_name = &artifact.export.name;

    // Determine which methods in this file should be flipped
    let source = fs::read_to_string(rs_path)
        .with_context(|| format!("Failed to read {}", rs_path.display()))?;

    // Parse to check which methods have actual bodies (no todo!())
    let file: syn::File = syn::parse_str(&source)
        .map_err(|e| anyhow::anyhow!("Failed to parse {}: {}", rs_path.display(), e))?;

    let impl_blocks: Vec<&syn::ItemImpl> = file
        .items
        .iter()
        .filter_map(|item| {
            if let syn::Item::Impl(imp) = item {
                if imp.trait_.is_none() {
                    if let syn::Type::Path(tp) = imp.self_ty.as_ref() {
                        if tp.path.is_ident(export_name) {
                            return Some(imp);
                        }
                    }
                }
            }
            None
        })
        .collect();

    // Collect implemented methods (those without todo!() as the sole body).
    // We use the raw source text to detect todo!() rather than syn's AST,
    // because syn::Stmt does not implement Debug without extra-traits.
    let method_bodies = crate::codegen::extract_method_bodies_from_source(&source);
    let mut implemented_methods: Vec<(String, bool)> = Vec::new(); // (name, is_public)
    for imp in &impl_blocks {
        for item in &imp.items {
            if let syn::ImplItem::Fn(method) = item {
                let name = method.sig.ident.to_string();
                let is_public = matches!(method.vis, syn::Visibility::Public(_));
                // If the method has a preserved body (i.e. it's not just todo!()), it's implemented
                if method_bodies.contains_key(&name) {
                    implemented_methods.push((name, is_public));
                }
            }
        }
    }

    // Build keys for implemented methods and flip stale → fresh
    let mut flipped = 0;
    for (method_name, is_public) in &implemented_methods {
        let keys = build_keys_for_method(artifact, export_name, method_name, *is_public);
        for key in keys {
            if let Some(entry) = tracker.artefacts.get(&key) {
                if entry.state == ArtefactState::Stale || entry.state == ArtefactState::NotImplemented {
                    tracker.mark_fresh(&key);
                    flipped += 1;
                }
            }
        }
    }

    if flipped > 0 {
        tracker.save(state_dir)?;
    }

    Ok(())
}

/// Build possible ArtefactKeys for a method given the artifact context.
fn build_keys_for_method(
    artifact: &PreparedArtifact,
    export_name: &str,
    method_name: &str,
    is_public: bool,
) -> Vec<crate::build_tracker::keys::ArtefactKey> {
    use crate::build_tracker::keys::ArtefactKey;

    let mut keys = Vec::new();

    match artifact.kind {
        ArtifactKind::Context => {
            if is_public {
                keys.push(ArtefactKey::functionality(export_name, method_name));
            } else {
                // Private method — try to match against known role methods
                for group in &artifact.role_methods {
                    for rm in &group.methods {
                        if rm.canonical_name == method_name {
                            keys.push(ArtefactKey::role_method(export_name, &group.role, &rm.name));
                        }
                    }
                }
            }
        }
        ArtifactKind::Data => {
            if is_public {
                keys.push(ArtefactKey::data_operation(export_name, method_name));
            }
        }
        ArtifactKind::Projection => {
            if is_public {
                keys.push(ArtefactKey::projection_functionality(export_name, method_name));
            } else {
                // Private method — match against the projection's role methods.
                for group in &artifact.role_methods {
                    for rm in &group.methods {
                        if rm.canonical_name == method_name {
                            keys.push(ArtefactKey::projection_role_method(export_name, &group.role, &rm.name));
                        }
                    }
                }
            }
        }
        _ => {}
    }

    keys
}

// ── Web-app structural verification (Phase 10) ─────────────────

/// Check tracker-vs-source reconciliation: detect drift where tracker
/// state disagrees with the actual source content.
pub fn verify_tracker_consistency(
    state_dir: &Path,
    workspace_root: &Path,
) -> VerifyResult {
    use crate::build_tracker::{ArtefactState, BuildTracker};

    let mut discrepancies = Vec::new();
    let tracker = match BuildTracker::load(state_dir) {
        Ok(t) => t,
        Err(_) => return VerifyResult { discrepancies },
    };

    if tracker.artefacts.is_empty() {
        return VerifyResult { discrepancies };
    }

    // For method-level entries, check if the source file exists and whether
    // the body contains todo!() when marked fresh, or has real code when marked not_implemented.
    // This is a heuristic check — we scan for todo!() as a proxy for "not implemented."
    for (key, entry) in &tracker.artefacts {
        let key_str = key.to_string();

        // Determine the source file path from the key
        let source_path = match key {
            crate::build_tracker::keys::ArtefactKey::Functionality { context, name } => {
                Some(workspace_root.join("src/contexts").join(format!("{}.rs", context)))
            }
            crate::build_tracker::keys::ArtefactKey::RoleMethod { context, .. } => {
                Some(workspace_root.join("src/contexts").join(format!("{}.rs", context)))
            }
            crate::build_tracker::keys::ArtefactKey::DataOperation { data, .. }
            | crate::build_tracker::keys::ArtefactKey::DataConstructor { data }
            | crate::build_tracker::keys::ArtefactKey::DataGetter { data, .. } => {
                Some(workspace_root.join("src/data").join(format!("{}.rs", data)))
            }
            crate::build_tracker::keys::ArtefactKey::ProjectionFunctionality { projection, .. }
            | crate::build_tracker::keys::ArtefactKey::ProjectionRoleMethod { projection, .. } => {
                Some(workspace_root.join("src/projections").join(format!("{}.rs", projection)))
            }
            _ => None, // Web entries and type entries don't have method bodies to check
        };

        if let Some(path) = source_path {
            if !path.is_file() {
                continue; // File doesn't exist yet — not a drift
            }
            if let Ok(source) = fs::read_to_string(&path) {
                let has_todo = source.contains("todo!()");

                if entry.state == ArtefactState::Fresh && has_todo {
                    discrepancies.push(Discrepancy {
                        kind: DiscrepancyKind::MissingMethod,
                        message: format!(
                            "the {} `{}` is recorded as fresh in the build tracker but its source file still contains a todo!() placeholder",
                            entry.kind.as_str(), key_str
                        ),
                    });
                }
            }
        }
    }

    VerifyResult { discrepancies }
}

/// Check that every declared route in the routing table is reachable from
/// the server's dispatch table (not hidden behind a catch-all todo!()).
pub fn verify_route_reachability(
    server_main_path: &Path,
    routing_artifacts: &[&PreparedArtifact],
) -> VerifyResult {
    let mut discrepancies = Vec::new();

    if !server_main_path.is_file() {
        return VerifyResult { discrepancies };
    }

    let source = match fs::read_to_string(server_main_path) {
        Ok(s) => s,
        Err(_) => return VerifyResult { discrepancies },
    };

    // Check if the dispatch function has a catch-all todo!()
    let has_catchall_todo = source.contains("todo!(\"dispatch route to component\")");

    if has_catchall_todo {
        for rt in routing_artifacts {
            for route in &rt.routes {
                discrepancies.push(Discrepancy {
                    kind: DiscrepancyKind::MissingMethod,
                    message: format!(
                        "the route `{}` is declared in the routing table but is not reachable from the server dispatch — \
                         the dispatch table has a catch-all todo!() instead of per-route arms",
                        route.path
                    ),
                });
            }
        }
    }

    VerifyResult { discrepancies }
}

/// Check that every component event binding to a server_only context has
/// both a server handler and a client stub emitted.
pub fn verify_rpc_edge_completeness(
    workspace_root: &Path,
    component_artifacts: &[&PreparedArtifact],
    context_artifacts: &[&PreparedArtifact],
) -> VerifyResult {
    use crate::codegen::to_snake_case;

    let mut discrepancies = Vec::new();

    // Find server_only contexts
    let server_only_contexts: BTreeSet<String> = context_artifacts
        .iter()
        .filter(|a| a.realisation_target.as_deref() == Some("server_only"))
        .map(|a| a.export.name.clone())
        .collect();

    if server_only_contexts.is_empty() {
        return VerifyResult { discrepancies };
    }

    // Check each component's event bindings
    for comp in component_artifacts {
        if let Some(composition) = &comp.composition {
            for eb in &composition.event_bindings {
                if server_only_contexts.contains(&eb.context) {
                    let ctx_snake = to_snake_case(&eb.context);
                    // Look up default functionality
                    let func_snake = context_artifacts
                        .iter()
                        .find(|a| a.export.name == eb.context)
                        .and_then(|a| a.functionalities.first())
                        .map(|f| to_snake_case(&f.name))
                        .unwrap_or_else(|| "unknown".to_string());

                    let expected_path = format!("/_rpc/{}/{}", ctx_snake, func_snake);

                    // Check server handler exists
                    let server_main = workspace_root.join("crates")
                        .read_dir().ok()
                        .and_then(|mut entries| {
                            entries.find_map(|e| {
                                let path = e.ok()?.path();
                                if path.file_name()?.to_str()?.ends_with("-server") {
                                    Some(path.join("src/main.rs"))
                                } else {
                                    None
                                }
                            })
                        });

                    if let Some(main_path) = &server_main {
                        if main_path.is_file() {
                            if let Ok(source) = fs::read_to_string(main_path) {
                                if !source.contains(&expected_path) {
                                    discrepancies.push(Discrepancy {
                                        kind: DiscrepancyKind::MissingMethod,
                                        message: format!(
                                            "the component `{}` binds its `{}` event to `{}/{}` (server_only), \
                                             but no RPC edge has been emitted — expected `{}` on the server",
                                            comp.export.name, eb.event, eb.context, func_snake, expected_path
                                        ),
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    VerifyResult { discrepancies }
}

/// Check that every component with Composition children actually constructs
/// and renders those children in its render body. Bodies containing only
/// todo!() are not violations (they're tracked work items). The check fires
/// on bodies that look real but miss a declared child.
pub fn verify_composition_actualisation(
    component_file: &Path,
    component_artifact: &PreparedArtifact,
) -> VerifyResult {
    let mut discrepancies = Vec::new();

    let composition = match &component_artifact.composition {
        Some(c) if !c.child_bindings.is_empty() => c,
        _ => return VerifyResult { discrepancies },
    };

    if !component_file.is_file() {
        return VerifyResult { discrepancies };
    }

    let source = match fs::read_to_string(component_file) {
        Ok(s) => s,
        Err(_) => return VerifyResult { discrepancies },
    };

    // If the body contains todo!(), it's an unfilled work item — not a violation
    if source.contains("todo!(") {
        return VerifyResult { discrepancies };
    }

    // Check each declared child is referenced in the source
    for cb in &composition.child_bindings {
        if !source.contains(&cb.child) {
            discrepancies.push(Discrepancy {
                kind: DiscrepancyKind::MissingMethod,
                message: format!(
                    "the component `{}` declares `{}` in its Composition, but `{}` is not constructed or \
                     rendered in {}",
                    component_artifact.export.name,
                    cb.child,
                    cb.child,
                    component_file.display()
                ),
            });
        }
    }

    VerifyResult { discrepancies }
}

// ── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prepared::*;

    fn money_transfer_artifact() -> PreparedArtifact {
        let mut a = PreparedArtifact::new(
            ArtifactKind::Context,
            "contexts/money_transfer.md".into(),
            "MoneyTransfer".into(),
            "MoneyTransfer".into(),
        );
        a.role_players = vec![
            RolePlayerSpec { name: "source".into(), data_type: "Account".into(), responsibility: "withdraw".into(), ..Default::default() },
            RolePlayerSpec { name: "sink".into(), data_type: "Account".into(), responsibility: "deposit".into(), ..Default::default() },
        ];
        a.props = vec![
            PropSpec { name: "amount".into(), description: "transfer amount".into() },
        ];
        a.functionalities = vec![
            FunctionalitySpec { name: "transfer".into(), description: "do it".into(), role_method_refs: vec![], ..Default::default() },
        ];
        a.role_methods = vec![
            RoleMethodGroupSpec {
                role: "source".into(),
                methods: vec![RoleMethodSpec {
                    name: "withdraw".into(), canonical_name: "source_withdraw".into(),
                    signature_hint: None, citation: "".into(),
                }],
            },
            RoleMethodGroupSpec {
                role: "sink".into(),
                methods: vec![RoleMethodSpec {
                    name: "deposit".into(), canonical_name: "sink_deposit".into(),
                    signature_hint: None, citation: "".into(),
                }],
            },
        ];
        a
    }

    #[test]
    fn verify_accepts_matching_source() {
        let source = r#"
pub struct MoneyTransfer {
    source: Account,
    sink: Account,
    amount: (),
}

impl MoneyTransfer {
    pub fn transfer(&mut self) {
        todo!()
    }

    fn source_withdraw(&self, source_: &Account) {
        todo!()
    }

    fn sink_deposit(&self, sink_: &Account) {
        todo!()
    }
}
"#;
        let result = verify_source(source, &money_transfer_artifact()).unwrap();
        assert!(result.is_ok(), "expected no discrepancies: {}", result.render());
    }

    #[test]
    fn verify_rejects_missing_field() {
        let source = r#"
pub struct MoneyTransfer {
    source: Account,
    // sink is missing
    amount: (),
}

impl MoneyTransfer {
    pub fn transfer(&mut self) { todo!() }
    fn source_withdraw(&self, source_: &Account) { todo!() }
    fn sink_deposit(&self, sink_: &Account) { todo!() }
}
"#;
        let result = verify_source(source, &money_transfer_artifact()).unwrap();
        assert!(!result.is_ok());
        assert!(result.discrepancies.iter().any(|d|
            d.kind == DiscrepancyKind::MissingField && d.message.contains("sink")
        ), "{}", result.render());
    }

    #[test]
    fn verify_rejects_missing_method() {
        let source = r#"
pub struct MoneyTransfer {
    source: Account,
    sink: Account,
    amount: (),
}

impl MoneyTransfer {
    pub fn transfer(&mut self) { todo!() }
    fn source_withdraw(&self, source_: &Account) { todo!() }
    // sink_deposit is missing
}
"#;
        let result = verify_source(source, &money_transfer_artifact()).unwrap();
        assert!(!result.is_ok());
        assert!(result.discrepancies.iter().any(|d|
            d.kind == DiscrepancyKind::MissingMethod && d.message.contains("sink_deposit")
        ), "{}", result.render());
    }

    #[test]
    fn verify_rejects_extra_field_on_data() {
        // Extra fields are checked on data artifacts (not contexts/projections,
        // which need internal bookkeeping).
        let mut a = PreparedArtifact::new(
            ArtifactKind::Data, "data/Amount.md".into(), "Amount".into(), "Amount".into(),
        );
        a.attributes = vec![
            AttributeSpec { name: "value".into(), description: "cents".into(), ..Default::default() },
        ];

        let source = r#"
pub struct Amount {
    pub value: u64,
    pub extra_field: String,
}
"#;
        let result = verify_source(source, &a).unwrap();
        assert!(!result.is_ok());
        assert!(result.discrepancies.iter().any(|d|
            d.kind == DiscrepancyKind::ExtraField && d.message.contains("extra_field")
        ), "{}", result.render());
    }

    #[test]
    fn verify_allows_extra_fields_on_contexts() {
        // Contexts may carry internal bookkeeping fields (caches, counters)
        // beyond role players and props.
        let source = r#"
pub struct MoneyTransfer {
    source: Account,
    sink: Account,
    amount: (),
    extra_bookkeeping: Vec<u8>,
}

impl MoneyTransfer {
    pub fn transfer(&mut self) { todo!() }
    fn source_withdraw(&self, source_: &Account) { todo!() }
    fn sink_deposit(&self, sink_: &Account) { todo!() }
}
"#;
        let result = verify_source(source, &money_transfer_artifact()).unwrap();
        assert!(result.is_ok(), "extra context fields should be allowed: {}", result.render());
    }

    #[test]
    fn verify_allows_extra_private_helpers() {
        // Plan says: "verifier checks ⊇ for private method set, not ="
        let source = r#"
pub struct MoneyTransfer {
    source: Account,
    sink: Account,
    amount: (),
}

impl MoneyTransfer {
    pub fn transfer(&mut self) { todo!() }
    fn source_withdraw(&self, source_: &Account) { todo!() }
    fn sink_deposit(&self, sink_: &Account) { todo!() }
    fn helper(&self) {} // extra private helper — allowed
}
"#;
        let result = verify_source(source, &money_transfer_artifact()).unwrap();
        assert!(result.is_ok(), "extra private helpers should be allowed: {}", result.render());
    }

    #[test]
    fn verify_data_artifact() {
        let mut a = PreparedArtifact::new(
            ArtifactKind::Data, "data/Amount.md".into(), "Amount".into(), "Amount".into(),
        );
        a.attributes = vec![
            AttributeSpec { name: "value".into(), description: "cents".into(), ..Default::default() },
        ];
        a.operations = vec![
            OperationSpec { name: "add".into(), description: "add".into(), kind: None, ..Default::default() },
        ];

        let source = r#"
pub struct Amount {
    pub value: u64,
}
impl Amount {
    pub fn add(&self) { todo!() }
}
"#;
        let result = verify_source(source, &a).unwrap();
        assert!(result.is_ok(), "{}", result.render());
    }
}
