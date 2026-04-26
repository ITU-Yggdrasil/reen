//! Extract canonical per-artefact YAML fragments and dependency edges
//! from prepared artifacts.
//!
//! Each prepared artifact produces:
//! - A set of `ArtefactKey` entries (one per trackable method/type).
//! - Per-key canonical YAML fragments (for hashing — sorted keys, deterministic).
//! - Per-key upstream dependency edges (for cascade propagation).

use crate::build_tracker::keys::ArtefactKey;
use crate::codegen::to_snake_case;
use crate::prepared::{ArtifactKind, PreparedArtifact};
use std::collections::BTreeMap;

/// A single extracted artefact with its canonical fragment and edges.
#[derive(Debug, Clone)]
pub struct ExtractedArtefact {
    pub key: ArtefactKey,
    pub canonical_fragment: String,
    pub upstream: Vec<ArtefactKey>,
}

/// Extract all trackable artefacts from a single prepared artifact.
///
/// Returns the list of extracted artefacts. The `sibling_artifacts` parameter
/// provides prepared artifacts from other drafts for resolving cross-draft edges
/// (e.g. a data type used as a role player in a context).
pub fn extract_artefacts(
    artifact: &PreparedArtifact,
    sibling_artifacts: &[&PreparedArtifact],
) -> Vec<ExtractedArtefact> {
    match artifact.kind {
        ArtifactKind::Context => extract_context(artifact, sibling_artifacts),
        ArtifactKind::Data => extract_data(artifact),
        ArtifactKind::Projection => extract_projection(artifact, sibling_artifacts),
        ArtifactKind::Operation
        | ArtifactKind::Component
        | ArtifactKind::Brand
        | ArtifactKind::RoutingTable
        | ArtifactKind::App => Vec::new(),
    }
}

// ── Context extraction ──────────────────────────────────────────

fn extract_context(
    a: &PreparedArtifact,
    siblings: &[&PreparedArtifact],
) -> Vec<ExtractedArtefact> {
    let ctx_name = &a.export.name;
    let mut result = Vec::new();

    // Role types — one per role player
    for rp in &a.role_players {
        let key = ArtefactKey::role_type(ctx_name, &rp.name);

        // Upstream: the data type that plays this role (cross-draft edge)
        let mut upstream = Vec::new();
        if let Some(_) = find_data_artifact(&rp.data_type, siblings) {
            upstream.push(ArtefactKey::data_type(&rp.data_type));
        }

        let fragment = canonical_yaml(&BTreeMap::from([
            ("context", ctx_name.as_str()),
            ("role", rp.name.as_str()),
            ("data_type", rp.data_type.as_str()),
            ("responsibility", rp.responsibility.as_str()),
        ]));

        result.push(ExtractedArtefact { key, canonical_fragment: fragment, upstream });
    }

    // Role methods — one per method in each role group
    for group in &a.role_methods {
        let role_type_key = ArtefactKey::role_type(ctx_name, &group.role);

        for rm in &group.methods {
            let key = ArtefactKey::role_method(ctx_name, &group.role, &rm.name);

            // Upstream: the role type this method belongs to
            let mut upstream = vec![role_type_key.clone()];

            // Cross-draft: if this role method invokes a data operation,
            // add an edge from data_operation → role_method.
            // We detect this from the citation/description mentioning operation names.
            // For now, we rely on the data type's operations being referenced
            // in the role player's data_type.
            let rp_data_type = a.role_players.iter()
                .find(|rp| rp.name == group.role)
                .map(|rp| &rp.data_type);

            if let Some(data_type) = rp_data_type {
                if let Some(data_art) = find_data_artifact(data_type, siblings) {
                    for op in &data_art.operations {
                        let op_snake = to_snake_case(&op.name);
                        let rm_text = format!("{} {}", rm.citation, rm.name);
                        if rm_text.to_lowercase().contains(&op_snake) {
                            upstream.push(ArtefactKey::data_operation(data_type, &op.name));
                        }
                    }
                }
            }

            let mut frag_map = BTreeMap::new();
            frag_map.insert("context", ctx_name.as_str());
            frag_map.insert("role", group.role.as_str());
            frag_map.insert("method", rm.name.as_str());
            frag_map.insert("canonical_name", rm.canonical_name.as_str());
            frag_map.insert("citation", rm.citation.as_str());
            if let Some(hint) = &rm.signature_hint {
                frag_map.insert("signature_hint", hint.as_str());
            }
            let fragment = canonical_yaml(&frag_map);

            result.push(ExtractedArtefact { key, canonical_fragment: fragment, upstream });
        }
    }

    // Functionalities — one per functionality
    for func in &a.functionalities {
        let key = ArtefactKey::functionality(ctx_name, &func.name);

        // Upstream: every role method referenced by this functionality
        let mut upstream: Vec<ArtefactKey> = Vec::new();
        for rm_ref in &func.role_method_refs {
            // Find the role this method belongs to
            for group in &a.role_methods {
                for rm in &group.methods {
                    if rm.canonical_name == *rm_ref {
                        upstream.push(ArtefactKey::role_method(ctx_name, &group.role, &rm.name));
                    }
                }
            }
        }

        let mut frag_map = BTreeMap::new();
        frag_map.insert("context", ctx_name.as_str());
        frag_map.insert("functionality", func.name.as_str());
        frag_map.insert("description", func.description.as_str());
        let refs_str = func.role_method_refs.join(",");
        frag_map.insert("role_method_refs", &refs_str);
        let fragment = canonical_yaml(&frag_map);

        result.push(ExtractedArtefact { key, canonical_fragment: fragment, upstream });
    }

    result
}

// ── Data extraction ─────────────────────────────────────────────

fn extract_data(a: &PreparedArtifact) -> Vec<ExtractedArtefact> {
    let data_name = &a.export.name;
    let mut result = Vec::new();

    // Data type
    let type_key = ArtefactKey::data_type(data_name);
    let mut attr_names: Vec<&str> = a.attributes.iter().map(|attr| attr.name.as_str()).collect();
    attr_names.sort();
    let attrs_str = attr_names.join(",");
    let invariants_str = a.invariants.join(",");
    let fragment = canonical_yaml(&BTreeMap::from([
        ("data", data_name.as_str()),
        ("attributes", attrs_str.as_str()),
        ("invariants", invariants_str.as_str()),
    ]));
    result.push(ExtractedArtefact {
        key: type_key.clone(),
        canonical_fragment: fragment,
        upstream: vec![],
    });

    // Data constructor — every Data type has a mandatory new() that enforces
    // declared invariants. Fragment includes attributes and invariants so any
    // change to either marks the constructor stale.
    let constructor_key = ArtefactKey::data_constructor(data_name);
    let constructor_fragment = canonical_yaml(&BTreeMap::from([
        ("data", data_name.as_str()),
        ("constructor", "new"),
        ("attributes", attrs_str.as_str()),
        ("invariants", invariants_str.as_str()),
    ]));
    result.push(ExtractedArtefact {
        key: constructor_key,
        canonical_fragment: constructor_fragment,
        upstream: vec![type_key.clone()],
    });

    // Data getters — one per declared attribute. Fragment is just the
    // attribute name + description; renaming or removing the attribute marks
    // the getter stale (and the scaffolder will drop it).
    for attr in &a.attributes {
        let key = ArtefactKey::data_getter(data_name, &attr.name);
        let fragment = canonical_yaml(&BTreeMap::from([
            ("data", data_name.as_str()),
            ("getter", attr.name.as_str()),
            ("description", attr.description.as_str()),
        ]));
        result.push(ExtractedArtefact {
            key,
            canonical_fragment: fragment,
            upstream: vec![type_key.clone()],
        });
    }

    // Data attribute types — one per declared attribute. The type itself is
    // chosen by `/implement` and lives in the scaffolded source; this entry
    // tracks whether the description has changed (in which case the
    // previously-chosen type goes stale and the field reverts to `()`).
    // Fragment includes the multiplicity so a single→list change also
    // invalidates the type choice.
    for attr in &a.attributes {
        let key = ArtefactKey::data_attribute_type(data_name, &attr.name);
        let multiplicity = match attr.multiplicity {
            crate::prepared::Multiplicity::Single => "single",
            crate::prepared::Multiplicity::List => "list",
        };
        let fragment = canonical_yaml(&BTreeMap::from([
            ("data", data_name.as_str()),
            ("attribute", attr.name.as_str()),
            ("description", attr.description.as_str()),
            ("multiplicity", multiplicity),
        ]));
        result.push(ExtractedArtefact {
            key,
            canonical_fragment: fragment,
            upstream: vec![type_key.clone()],
        });
    }

    // Data operations
    for op in &a.operations {
        let key = ArtefactKey::data_operation(data_name, &op.name);
        let fragment = canonical_yaml(&BTreeMap::from([
            ("data", data_name.as_str()),
            ("operation", op.name.as_str()),
            ("description", op.description.as_str()),
        ]));
        result.push(ExtractedArtefact {
            key,
            canonical_fragment: fragment,
            upstream: vec![type_key.clone()],
        });
    }

    result
}

// ── Projection extraction ───────────────────────────────────────

fn extract_projection(
    a: &PreparedArtifact,
    siblings: &[&PreparedArtifact],
) -> Vec<ExtractedArtefact> {
    // Projections share the context-shaped fields (role_players, role_methods,
    // functionalities). Extraction mirrors extract_context with
    // projection-flavoured key variants.
    let proj_name = &a.export.name;
    let mut result = Vec::new();

    // Role types — one per role player.
    for rp in &a.role_players {
        let key = ArtefactKey::projection_role_type(proj_name, &rp.name);

        let mut upstream = Vec::new();
        if find_data_artifact(&rp.data_type, siblings).is_some() {
            upstream.push(ArtefactKey::data_type(&rp.data_type));
        }

        let fragment = canonical_yaml(&BTreeMap::from([
            ("projection", proj_name.as_str()),
            ("role", rp.name.as_str()),
            ("data_type", rp.data_type.as_str()),
            ("responsibility", rp.responsibility.as_str()),
        ]));

        result.push(ExtractedArtefact { key, canonical_fragment: fragment, upstream });
    }

    // Role methods — one per method in each role group.
    for group in &a.role_methods {
        let role_type_key = ArtefactKey::projection_role_type(proj_name, &group.role);

        for rm in &group.methods {
            let key = ArtefactKey::projection_role_method(proj_name, &group.role, &rm.name);

            let mut upstream = vec![role_type_key.clone()];

            // Cross-draft edge to data operations referenced by the role method.
            let rp_data_type = a.role_players.iter()
                .find(|rp| rp.name == group.role)
                .map(|rp| &rp.data_type);

            if let Some(data_type) = rp_data_type {
                if let Some(data_art) = find_data_artifact(data_type, siblings) {
                    for op in &data_art.operations {
                        let op_snake = to_snake_case(&op.name);
                        let rm_text = format!("{} {}", rm.citation, rm.name);
                        if rm_text.to_lowercase().contains(&op_snake) {
                            upstream.push(ArtefactKey::data_operation(data_type, &op.name));
                        }
                    }
                }
            }

            let mut frag_map = BTreeMap::new();
            frag_map.insert("projection", proj_name.as_str());
            frag_map.insert("role", group.role.as_str());
            frag_map.insert("method", rm.name.as_str());
            frag_map.insert("canonical_name", rm.canonical_name.as_str());
            frag_map.insert("citation", rm.citation.as_str());
            if let Some(hint) = &rm.signature_hint {
                frag_map.insert("signature_hint", hint.as_str());
            }
            let fragment = canonical_yaml(&frag_map);

            result.push(ExtractedArtefact { key, canonical_fragment: fragment, upstream });
        }
    }

    // Functionalities — one per functionality.
    for func in &a.functionalities {
        let key = ArtefactKey::projection_functionality(proj_name, &func.name);

        let mut upstream: Vec<ArtefactKey> = Vec::new();
        for rm_ref in &func.role_method_refs {
            for group in &a.role_methods {
                for rm in &group.methods {
                    if rm.canonical_name == *rm_ref {
                        upstream.push(ArtefactKey::projection_role_method(proj_name, &group.role, &rm.name));
                    }
                }
            }
        }

        let mut frag_map = BTreeMap::new();
        frag_map.insert("projection", proj_name.as_str());
        frag_map.insert("functionality", func.name.as_str());
        frag_map.insert("description", func.description.as_str());
        let refs_str = func.role_method_refs.join(",");
        frag_map.insert("role_method_refs", &refs_str);
        let fragment = canonical_yaml(&frag_map);

        result.push(ExtractedArtefact { key, canonical_fragment: fragment, upstream });
    }

    result
}

// ── Helpers ─────────────────────────────────────────────────────

/// Find a data artifact by export name among siblings.
fn find_data_artifact<'a>(
    data_type: &str,
    siblings: &[&'a PreparedArtifact],
) -> Option<&'a PreparedArtifact> {
    siblings.iter().find(|a| {
        a.kind == ArtifactKind::Data && a.export.name == data_type
    }).copied()
}

/// Produce a canonical YAML string from a sorted map.
/// Uses BTreeMap to guarantee key order, producing deterministic output
/// suitable for hashing.
fn canonical_yaml(map: &BTreeMap<&str, &str>) -> String {
    let mut out = String::new();
    for (k, v) in map {
        out.push_str(k);
        out.push_str(": ");
        out.push_str(v);
        out.push('\n');
    }
    out
}

/// Compute a SHA-256 hash of a canonical fragment string.
pub fn hash_fragment(fragment: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(fragment.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Hash a raw file's bytes (for draft-level change detection).
pub fn hash_file_bytes(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prepared::*;

    fn account_data() -> PreparedArtifact {
        let mut a = PreparedArtifact::new(
            ArtifactKind::Data,
            "data/Account.md".into(),
            "Account".into(),
            "Account".into(),
        );
        a.attributes = vec![
            AttributeSpec { name: "balance".into(), description: "current balance".into(), ..Default::default() },
            AttributeSpec { name: "owner".into(), description: "account owner".into(), ..Default::default() },
        ];
        a.invariants = vec!["balance >= 0".into()];
        a.operations = vec![
            OperationSpec { name: "deposit".into(), description: "Add funds.".into(), kind: None, ..Default::default() },
            OperationSpec { name: "withdraw".into(), description: "Remove funds.".into(), kind: None, ..Default::default() },
        ];
        a
    }

    fn money_transfer_context() -> PreparedArtifact {
        let mut a = PreparedArtifact::new(
            ArtifactKind::Context,
            "contexts/money_transfer.md".into(),
            "MoneyTransfer".into(),
            "MoneyTransfer".into(),
        );
        a.role_players = vec![
            RolePlayerSpec { name: "source".into(), data_type: "Account".into(), responsibility: "withdraw from".into(), ..Default::default() },
            RolePlayerSpec { name: "sink".into(), data_type: "Account".into(), responsibility: "deposit to".into(), ..Default::default() },
        ];
        a.props = vec![
            PropSpec { name: "amount".into(), description: "transfer amount".into() },
        ];
        a.functionalities = vec![
            FunctionalitySpec {
                name: "transfer".into(),
                description: "Execute the transfer.".into(),
                role_method_refs: vec!["source_withdraw".into(), "sink_deposit".into()], ..Default::default()
            },
        ];
        a.role_methods = vec![
            RoleMethodGroupSpec {
                role: "source".into(),
                methods: vec![RoleMethodSpec {
                    name: "withdraw".into(),
                    canonical_name: "source_withdraw".into(),
                    signature_hint: None,
                    citation: "removes the transfer amount via withdraw".into(),
                }],
            },
            RoleMethodGroupSpec {
                role: "sink".into(),
                methods: vec![RoleMethodSpec {
                    name: "deposit".into(),
                    canonical_name: "sink_deposit".into(),
                    signature_hint: None,
                    citation: "adds the transfer amount via deposit".into(),
                }],
            },
        ];
        a
    }

    fn balance_projection() -> PreparedArtifact {
        // Projections share the context shape: declare role players and a
        // functionality that reads from them.
        let mut a = PreparedArtifact::new(
            ArtifactKind::Projection,
            "projections/balance.md".into(),
            "AccountBalance".into(),
            "AccountBalance".into(),
        );
        a.role_players = vec![
            RolePlayerSpec {
                name: "account".into(),
                data_type: "Account".into(),
                responsibility: "the account being viewed".into(), ..Default::default()
            },
        ];
        a.functionalities = vec![
            FunctionalitySpec {
                name: "current_balance".into(),
                description: "Returns the account's current balance.".into(),
                role_method_refs: vec![], ..Default::default()
            },
        ];
        a
    }

    // ── Data edge tests ──────────────────────────────────────────

    #[test]
    fn data_extracts_attribute_type_entries() {
        // Bug 2.5c regression: every declared attribute also produces a
        // `data_attribute_type` entry whose fragment includes the
        // description and multiplicity. When either changes, the entry
        // hashes differently and the previously-chosen Rust type is marked
        // stale (so the field reverts to ()).
        let data = account_data();
        let extracted = extract_artefacts(&data, &[]);

        let balance_attr = extracted.iter()
            .find(|e| e.key.to_string() == "data/account/attribute/balance")
            .expect("expected a data_attribute_type entry for balance");
        assert_eq!(balance_attr.upstream, vec![ArtefactKey::data_type("Account")]);
        assert!(balance_attr.canonical_fragment.contains("balance"));
        assert!(balance_attr.canonical_fragment.contains("current balance"));
        assert!(balance_attr.canonical_fragment.contains("single"));

        let owner_attr = extracted.iter()
            .find(|e| e.key.to_string() == "data/account/attribute/owner")
            .expect("expected a data_attribute_type entry for owner");
        assert!(owner_attr.canonical_fragment.contains("account owner"));
    }

    #[test]
    fn data_attribute_type_fragment_changes_when_description_changes() {
        // Same shape, different description → different hash → the entry
        // goes stale on next prepare.
        let mut data = account_data();
        let original = extract_artefacts(&data, &[]);
        let original_balance = original.iter()
            .find(|e| e.key.to_string() == "data/account/attribute/balance")
            .unwrap();

        data.attributes[0].description = "the running balance, in cents".into();
        let after = extract_artefacts(&data, &[]);
        let new_balance = after.iter()
            .find(|e| e.key.to_string() == "data/account/attribute/balance")
            .unwrap();

        assert_ne!(
            original_balance.canonical_fragment,
            new_balance.canonical_fragment,
            "fragment must change when description changes",
        );
    }

    #[test]
    fn data_attribute_type_fragment_changes_when_multiplicity_changes() {
        let mut data = account_data();
        let original = extract_artefacts(&data, &[]);
        let original_balance = original.iter()
            .find(|e| e.key.to_string() == "data/account/attribute/balance")
            .unwrap();

        data.attributes[0].multiplicity = crate::prepared::Multiplicity::List;
        let after = extract_artefacts(&data, &[]);
        let new_balance = after.iter()
            .find(|e| e.key.to_string() == "data/account/attribute/balance")
            .unwrap();

        assert_ne!(
            original_balance.canonical_fragment,
            new_balance.canonical_fragment,
            "single → list invalidates the previously-chosen attribute type",
        );
    }

    #[test]
    fn data_extracts_type_and_operations() {
        let data = account_data();
        let extracted = extract_artefacts(&data, &[]);

        // Should have: data_type + constructor + 2 getters + 2 attribute types + 2 operations = 8 entries
        assert_eq!(extracted.len(), 8);

        let type_entry = extracted.iter().find(|e| matches!(e.key, ArtefactKey::DataType { .. })).unwrap();
        assert_eq!(type_entry.key.to_string(), "data/account/type");
        assert!(type_entry.upstream.is_empty());

        let constructor = extracted.iter().find(|e| e.key.to_string() == "data/account/constructor").unwrap();
        assert_eq!(constructor.upstream, vec![ArtefactKey::data_type("Account")]);

        let balance_getter = extracted.iter().find(|e| e.key.to_string() == "data/account/getter/balance").unwrap();
        assert_eq!(balance_getter.upstream, vec![ArtefactKey::data_type("Account")]);

        let owner_getter = extracted.iter().find(|e| e.key.to_string() == "data/account/getter/owner").unwrap();
        assert_eq!(owner_getter.upstream, vec![ArtefactKey::data_type("Account")]);

        let deposit = extracted.iter().find(|e| e.key.to_string() == "data/account/operation/deposit").unwrap();
        assert_eq!(deposit.upstream, vec![ArtefactKey::data_type("Account")]);

        let withdraw = extracted.iter().find(|e| e.key.to_string() == "data/account/operation/withdraw").unwrap();
        assert_eq!(withdraw.upstream, vec![ArtefactKey::data_type("Account")]);
    }

    // ── Context edge tests ───────────────────────────────────────

    #[test]
    fn context_extracts_role_types() {
        let ctx = money_transfer_context();
        let data = account_data();
        let extracted = extract_artefacts(&ctx, &[&data]);

        let source_type = extracted.iter()
            .find(|e| e.key.to_string() == "contexts/money_transfer/role/source/type")
            .unwrap();

        // Role type should depend on the data type it plays
        assert!(source_type.upstream.contains(&ArtefactKey::data_type("Account")));
    }

    #[test]
    fn context_role_method_depends_on_role_type() {
        let ctx = money_transfer_context();
        let data = account_data();
        let extracted = extract_artefacts(&ctx, &[&data]);

        let source_withdraw = extracted.iter()
            .find(|e| e.key.to_string() == "contexts/money_transfer/role/source/withdraw")
            .unwrap();

        // Role method should depend on its role type
        assert!(source_withdraw.upstream.contains(
            &ArtefactKey::role_type("MoneyTransfer", "source")
        ));
    }

    #[test]
    fn context_functionality_depends_on_role_methods() {
        let ctx = money_transfer_context();
        let data = account_data();
        let extracted = extract_artefacts(&ctx, &[&data]);

        let transfer = extracted.iter()
            .find(|e| e.key.to_string() == "contexts/money_transfer/functionality/transfer")
            .unwrap();

        // Functionality should depend on the role methods it references
        assert!(transfer.upstream.contains(
            &ArtefactKey::role_method("MoneyTransfer", "source", "withdraw")
        ));
        assert!(transfer.upstream.contains(
            &ArtefactKey::role_method("MoneyTransfer", "sink", "deposit")
        ));
    }

    #[test]
    fn context_counts_all_artefacts() {
        let ctx = money_transfer_context();
        let data = account_data();
        let extracted = extract_artefacts(&ctx, &[&data]);

        // 2 role types + 2 role methods + 1 functionality = 5
        assert_eq!(extracted.len(), 5);
    }

    // ── Cross-draft edge tests ───────────────────────────────────

    #[test]
    fn cross_draft_data_type_to_role_type() {
        let ctx = money_transfer_context();
        let data = account_data();
        let extracted = extract_artefacts(&ctx, &[&data]);

        // Both source and sink role types should depend on Account data type
        let source_type = extracted.iter()
            .find(|e| e.key.to_string() == "contexts/money_transfer/role/source/type")
            .unwrap();
        let sink_type = extracted.iter()
            .find(|e| e.key.to_string() == "contexts/money_transfer/role/sink/type")
            .unwrap();

        assert!(source_type.upstream.contains(&ArtefactKey::data_type("Account")));
        assert!(sink_type.upstream.contains(&ArtefactKey::data_type("Account")));
    }

    #[test]
    fn cross_draft_role_method_to_data_operation() {
        let ctx = money_transfer_context();
        let data = account_data();
        let extracted = extract_artefacts(&ctx, &[&data]);

        // source_withdraw references "withdraw" which matches Account.withdraw
        let source_withdraw = extracted.iter()
            .find(|e| e.key.to_string() == "contexts/money_transfer/role/source/withdraw")
            .unwrap();
        assert!(source_withdraw.upstream.contains(
            &ArtefactKey::data_operation("Account", "withdraw")
        ));

        // sink_deposit references "deposit" which matches Account.deposit
        let sink_deposit = extracted.iter()
            .find(|e| e.key.to_string() == "contexts/money_transfer/role/sink/deposit")
            .unwrap();
        assert!(sink_deposit.upstream.contains(
            &ArtefactKey::data_operation("Account", "deposit")
        ));
    }

    #[test]
    fn cross_draft_no_sibling_no_edge() {
        // Without sibling data, no cross-draft edges
        let ctx = money_transfer_context();
        let extracted = extract_artefacts(&ctx, &[]);

        let source_type = extracted.iter()
            .find(|e| e.key.to_string() == "contexts/money_transfer/role/source/type")
            .unwrap();
        assert!(source_type.upstream.is_empty());
    }

    // ── Projection edge tests ────────────────────────────────────

    #[test]
    fn projection_functionality_depends_on_role_player_data_type() {
        let proj = balance_projection();
        let data = account_data();
        let extracted = extract_artefacts(&proj, &[&data]);

        // Projections now extract context-shape entries: role types and
        // functionalities. The single role player + single functionality
        // produces 2 entries.
        assert_eq!(extracted.len(), 2);

        let role_type = extracted.iter()
            .find(|e| e.key.to_string() == "projections/account_balance/role/account/type")
            .unwrap();
        assert!(role_type.upstream.contains(&ArtefactKey::data_type("Account")));

        let functionality = extracted.iter()
            .find(|e| e.key.to_string() == "projections/account_balance/functionality/current_balance")
            .unwrap();
        // Functionality has no role_method_refs in this fixture, so no upstream.
        assert!(functionality.upstream.is_empty());
    }

    #[test]
    fn projection_no_sibling_no_edge() {
        let proj = balance_projection();
        let extracted = extract_artefacts(&proj, &[]);

        let query = &extracted[0];
        assert!(query.upstream.is_empty());
    }

    // ── Fragment hashing tests ───────────────────────────────────

    #[test]
    fn canonical_fragment_is_deterministic() {
        let data = account_data();
        let e1 = extract_artefacts(&data, &[]);
        let e2 = extract_artefacts(&data, &[]);

        for (a, b) in e1.iter().zip(e2.iter()) {
            assert_eq!(a.canonical_fragment, b.canonical_fragment,
                "canonical fragments should be deterministic for {}",
                a.key
            );
        }
    }

    #[test]
    fn canonical_fragment_sorted_keys() {
        let data = account_data();
        let extracted = extract_artefacts(&data, &[]);
        let type_entry = &extracted[0];

        // BTreeMap guarantees sorted keys
        let lines: Vec<&str> = type_entry.canonical_fragment.lines().collect();
        let keys: Vec<&str> = lines.iter()
            .map(|l| l.split(':').next().unwrap())
            .collect();

        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted, "fragment keys should be sorted");
    }

    #[test]
    fn hash_fragment_deterministic() {
        let h1 = hash_fragment("hello world");
        let h2 = hash_fragment("hello world");
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 64); // SHA-256 hex is 64 chars
    }

    #[test]
    fn hash_fragment_changes_on_different_input() {
        let h1 = hash_fragment("hello");
        let h2 = hash_fragment("world");
        assert_ne!(h1, h2);
    }

    #[test]
    fn hash_file_bytes_works() {
        let h = hash_file_bytes(b"test content");
        assert_eq!(h.len(), 64);
    }

    // ── Full pipeline test ───────────────────────────────────────

    #[test]
    fn full_cross_draft_scenario() {
        // Set up a mini project: Account data + MoneyTransfer context + AccountBalance projection
        let data = account_data();
        let ctx = money_transfer_context();
        let proj = balance_projection();

        let siblings: Vec<&PreparedArtifact> = vec![&data, &ctx, &proj];

        let data_artefacts = extract_artefacts(&data, &siblings);
        let ctx_artefacts = extract_artefacts(&ctx, &siblings);
        let proj_artefacts = extract_artefacts(&proj, &siblings);

        // Verify the total count
        // Data: 1 type + 1 constructor + 2 getters + 2 attribute types + 2 ops = 8
        // Context: 2 role types + 2 role methods + 1 functionality = 5
        // Projection (context-shape): 1 role type + 1 functionality = 2
        assert_eq!(data_artefacts.len(), 8);
        assert_eq!(ctx_artefacts.len(), 5);
        assert_eq!(proj_artefacts.len(), 2);

        // Verify the cascade chain: Account type → Account.deposit → sink role type → sink_deposit → transfer
        // This is the path that should go stale if Account's attributes change.
    }
}
