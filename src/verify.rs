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
pub fn verify_file(rs_path: &Path, artifact: &PreparedArtifact) -> Result<VerifyResult> {
    let source = fs::read_to_string(rs_path)
        .with_context(|| format!("Failed to read {}", rs_path.display()))?;
    verify_source(&source, artifact)
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
            if artifact.source_data.is_some() {
                fields.insert("source".to_string());
            }
        }
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
            for query in &artifact.queries {
                pub_methods.insert(crate::codegen::to_snake_case(&query.name));
            }
        }
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
    }
}

fn pub_method_label(artifact: &PreparedArtifact, _name: &str) -> &'static str {
    match artifact.kind {
        ArtifactKind::Context => "functionality",
        ArtifactKind::Data => "operation",
        ArtifactKind::Projection => "query",
    }
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
            RolePlayerSpec { name: "source".into(), data_type: "Account".into(), responsibility: "withdraw".into() },
            RolePlayerSpec { name: "sink".into(), data_type: "Account".into(), responsibility: "deposit".into() },
        ];
        a.props = vec![
            PropSpec { name: "amount".into(), description: "transfer amount".into() },
        ];
        a.functionalities = vec![
            FunctionalitySpec { name: "transfer".into(), description: "do it".into(), role_method_refs: vec![] },
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
            AttributeSpec { name: "value".into(), description: "cents".into() },
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
            AttributeSpec { name: "value".into(), description: "cents".into() },
        ];
        a.operations = vec![
            OperationSpec { name: "add".into(), description: "add".into() },
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
