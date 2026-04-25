//! Kind-discipline structural checks.
//!
//! Layered on top of the existing structural verify pass, these checks enforce
//! the per-kind rules from `DCI.md` / `DRAFT_FORMAT.md`:
//!
//!   * Data — private fields, mandatory `new()`, getter per attribute, fields
//!     reference Data only.
//!   * Projection — private fields, private role methods, public surface =
//!     Functionalities, immutable receivers, fields reference Data/Projection.
//!   * Context — flagged separately by the auditor (soft warning).
//!
//! All checks are deterministic, parse Rust with `syn` (no regex), and emit
//! DCI-language messages. Each check has a locked-wording unit test.

use crate::prepared::{ArtifactKind, PreparedArtifact};
use crate::verify::{Discrepancy, DiscrepancyKind};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

/// Maps domain type names to their declared artefact kind.
///
/// Built from the prepared YAML files under `drafts/prepare/{contexts, data,
/// projections}/`. The verify pass consults the index to enforce field-type
/// containment rules (Data may not reference Projections or Contexts;
/// Projections may not reference Contexts).
#[derive(Debug, Default, Clone)]
pub struct KindIndex {
    types: BTreeMap<String, ArtifactKind>,
}

impl KindIndex {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, name: impl Into<String>, kind: ArtifactKind) {
        self.types.insert(name.into(), kind);
    }

    pub fn lookup(&self, type_name: &str) -> Option<ArtifactKind> {
        self.types.get(type_name).cloned()
    }

    /// Build from `drafts/prepare/{contexts, data, projections}/*.yml`.
    pub fn from_prepared_dir(prepared_root: &Path) -> anyhow::Result<Self> {
        let mut index = Self::empty();
        for subdir in &["contexts", "data", "projections"] {
            let dir = prepared_root.join(subdir);
            if !dir.is_dir() {
                continue;
            }
            for entry in fs::read_dir(&dir)? {
                let path = entry?.path();
                if path.extension().and_then(|e| e.to_str()) != Some("yml") {
                    continue;
                }
                let content = match fs::read_to_string(&path) {
                    Ok(c) => c,
                    Err(_) => continue,
                };
                let artifact: PreparedArtifact = match serde_yaml::from_str(&content) {
                    Ok(a) => a,
                    Err(_) => continue,
                };
                index.insert(artifact.export.name.clone(), artifact.kind);
            }
        }
        Ok(index)
    }
}

/// Run the kind-discipline checks against a parsed Rust file.
pub fn check(
    file: &syn::File,
    artifact: &PreparedArtifact,
    index: &KindIndex,
) -> Vec<Discrepancy> {
    let mut findings = Vec::new();
    match artifact.kind {
        ArtifactKind::Data => check_data(file, artifact, index, &mut findings),
        ArtifactKind::Projection => check_projection(file, artifact, index, &mut findings),
        _ => {}
    }
    findings
}

// ── Data checks (4–7) ─────────────────────────────────────────────

fn check_data(
    file: &syn::File,
    artifact: &PreparedArtifact,
    index: &KindIndex,
    findings: &mut Vec<Discrepancy>,
) {
    let name = &artifact.export.name;
    let Some(struct_item) = find_struct(file, name) else {
        return;
    };
    let impl_methods = collect_impl_methods(file, name);

    // (4) Data field visibility — every field must be private.
    let pub_fields: Vec<String> = struct_item
        .fields
        .iter()
        .filter(|f| matches!(f.vis, syn::Visibility::Public(_)))
        .filter_map(|f| f.ident.as_ref().map(|i| i.to_string()))
        .collect();
    if !pub_fields.is_empty() {
        findings.push(Discrepancy {
            kind: DiscrepancyKind::WrongVisibility,
            message: format!(
                "the data type `{}` declares fields with pub visibility ({}) — data fields must be private to preserve the immutability contract.",
                name,
                pub_fields.join(", "),
            ),
        });
    }

    // (5) Constructor presence — pub fn new must exist.
    let has_pub_new = impl_methods.iter().any(|(method_name, vis, _)| {
        method_name == "new" && matches!(vis, syn::Visibility::Public(_))
    });
    if !has_pub_new {
        findings.push(Discrepancy {
            kind: DiscrepancyKind::MissingMethod,
            message: format!(
                "the data type `{}` is missing its required `new()` constructor — every data type must have a constructor that enforces declared invariants.",
                name,
            ),
        });
    }

    // (6) Getter presence — every declared attribute must have a pub getter.
    let pub_method_names: BTreeSet<&String> = impl_methods
        .iter()
        .filter(|(_, vis, _)| matches!(vis, syn::Visibility::Public(_)))
        .map(|(n, _, _)| n)
        .collect();
    for attr in &artifact.attributes {
        let snake = crate::codegen::to_snake_case(&attr.name);
        if !pub_method_names.contains(&snake) {
            findings.push(Discrepancy {
                kind: DiscrepancyKind::MissingMethod,
                message: format!(
                    "the data type `{}` declares an `{}` attribute but the impl block has no `{}()` getter — every declared field must have a getter.",
                    name, attr.name, snake,
                ),
            });
        }
    }

    // (7) Field-type containment — non-primitive field types must resolve to
    // another Data type.
    for field in &struct_item.fields {
        for referenced in extract_type_names(&field.ty) {
            if is_primitive_or_std(&referenced) {
                continue;
            }
            match index.lookup(&referenced) {
                None => {} // External / not indexed — let other passes flag it.
                Some(ArtifactKind::Data) => {} // Allowed.
                Some(other_kind) => {
                    let field_name = field
                        .ident
                        .as_ref()
                        .map(|i| i.to_string())
                        .unwrap_or_else(|| "<unnamed>".to_string());
                    findings.push(Discrepancy {
                        kind: DiscrepancyKind::ExtraField,
                        message: format!(
                            "the data type `{}` has a field `{}: {}`, but `{}` is a {} — data types may only reference other data types.",
                            name,
                            field_name,
                            referenced,
                            referenced,
                            kind_label(other_kind),
                        ),
                    });
                }
            }
        }
    }
}

// ── Projection checks (8–12) ──────────────────────────────────────

fn check_projection(
    file: &syn::File,
    artifact: &PreparedArtifact,
    index: &KindIndex,
    findings: &mut Vec<Discrepancy>,
) {
    let name = &artifact.export.name;
    let Some(struct_item) = find_struct(file, name) else {
        return;
    };
    let impl_methods = collect_impl_methods(file, name);

    // (8) Projection field visibility — every field must be private.
    let pub_fields: Vec<String> = struct_item
        .fields
        .iter()
        .filter(|f| matches!(f.vis, syn::Visibility::Public(_)))
        .filter_map(|f| f.ident.as_ref().map(|i| i.to_string()))
        .collect();
    if !pub_fields.is_empty() {
        findings.push(Discrepancy {
            kind: DiscrepancyKind::WrongVisibility,
            message: format!(
                "the projection `{}` declares fields with pub visibility ({}) — projection fields must be private.",
                name,
                pub_fields.join(", "),
            ),
        });
    }

    // (9) Projection role method visibility — methods named in
    // role_method_refs must be private.
    let role_method_canonicals: BTreeSet<&str> = artifact
        .role_methods
        .iter()
        .flat_map(|g| g.methods.iter().map(|m| m.canonical_name.as_str()))
        .collect();
    for (method_name, vis, _) in &impl_methods {
        if role_method_canonicals.contains(method_name.as_str())
            && matches!(vis, syn::Visibility::Public(_))
        {
            findings.push(Discrepancy {
                kind: DiscrepancyKind::WrongVisibility,
                message: format!(
                    "the projection `{}` declares role method `{}` as `pub` — projection role methods must be private.",
                    name, method_name,
                ),
            });
        }
    }

    // (10) Public surface = declared functionalities (plus pub fn new).
    let declared_functionalities: BTreeSet<String> = artifact
        .functionalities
        .iter()
        .map(|f| crate::codegen::to_snake_case(&f.name))
        .collect();
    for (method_name, vis, _) in &impl_methods {
        if !matches!(vis, syn::Visibility::Public(_)) {
            continue;
        }
        if method_name == "new" {
            continue; // The mandatory constructor is always allowed pub.
        }
        if declared_functionalities.contains(method_name) {
            continue;
        }
        findings.push(Discrepancy {
            kind: DiscrepancyKind::WrongVisibility,
            message: format!(
                "the projection `{}` exposes `{}` as a public method, but it is not a declared functionality — only declared functionalities may be public on projections.",
                name, method_name,
            ),
        });
    }

    // (11) Projection immutability — every functionality takes &self;
    // every role method takes its role player as & (never &mut).
    for (method_name, _, sig) in &impl_methods {
        if takes_mut_self(sig) {
            // Functionality?
            if declared_functionalities.contains(method_name) {
                findings.push(Discrepancy {
                    kind: DiscrepancyKind::WrongVisibility,
                    message: format!(
                        "the projection `{}` declares functionality `{}` with `&mut self` — projection functionalities must take `&self`.",
                        name, method_name,
                    ),
                });
            } else {
                // Generic catch-all: no &mut self anywhere on the impl block.
                findings.push(Discrepancy {
                    kind: DiscrepancyKind::WrongVisibility,
                    message: format!(
                        "the projection `{}` declares method `{}` with `&mut self` — projections must not declare any `&mut self` method.",
                        name, method_name,
                    ),
                });
            }
        }
        // Role method role-player parameter must not be &mut.
        if role_method_canonicals.contains(method_name.as_str()) && takes_mut_role_player(sig) {
            findings.push(Discrepancy {
                kind: DiscrepancyKind::WrongVisibility,
                message: format!(
                    "the projection `{}` declares role method `{}` with the role player as `&mut` — projection role methods must not mutate role players.",
                    name, method_name,
                ),
            });
        }
    }

    // (12) Field-type containment — non-primitive field types must resolve to
    // another Data or Projection type.
    for field in &struct_item.fields {
        for referenced in extract_type_names(&field.ty) {
            if is_primitive_or_std(&referenced) {
                continue;
            }
            match index.lookup(&referenced) {
                None => {}
                Some(ArtifactKind::Data) | Some(ArtifactKind::Projection) => {}
                Some(other_kind) => {
                    let field_name = field
                        .ident
                        .as_ref()
                        .map(|i| i.to_string())
                        .unwrap_or_else(|| "<unnamed>".to_string());
                    findings.push(Discrepancy {
                        kind: DiscrepancyKind::ExtraField,
                        message: format!(
                            "the projection `{}` has a field `{}: {}`, but `{}` is a {} — projections may only reference data and other projections.",
                            name,
                            field_name,
                            referenced,
                            referenced,
                            kind_label(other_kind),
                        ),
                    });
                }
            }
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────

fn find_struct<'f>(file: &'f syn::File, name: &str) -> Option<&'f syn::ItemStruct> {
    file.items.iter().find_map(|item| {
        if let syn::Item::Struct(s) = item {
            if s.ident == name {
                return Some(s);
            }
        }
        None
    })
}

/// Collect (method_name, visibility, signature) tuples from every impl block
/// matching `target` (skipping trait impls).
fn collect_impl_methods<'f>(
    file: &'f syn::File,
    target: &str,
) -> Vec<(String, syn::Visibility, &'f syn::Signature)> {
    let mut out = Vec::new();
    for item in &file.items {
        if let syn::Item::Impl(imp) = item {
            if imp.trait_.is_some() {
                continue;
            }
            if let syn::Type::Path(tp) = imp.self_ty.as_ref() {
                if !tp.path.is_ident(target) {
                    continue;
                }
            } else {
                continue;
            }
            for impl_item in &imp.items {
                if let syn::ImplItem::Fn(method) = impl_item {
                    out.push((
                        method.sig.ident.to_string(),
                        method.vis.clone(),
                        &method.sig,
                    ));
                }
            }
        }
    }
    out
}

fn takes_mut_self(sig: &syn::Signature) -> bool {
    sig.inputs.iter().any(|arg| {
        matches!(
            arg,
            syn::FnArg::Receiver(syn::Receiver {
                reference: Some(_),
                mutability: Some(_),
                ..
            })
        )
    })
}

fn takes_mut_role_player(sig: &syn::Signature) -> bool {
    // Skip the receiver; check remaining inputs for `&mut <Type>` parameters.
    sig.inputs.iter().skip(1).any(|arg| {
        if let syn::FnArg::Typed(pat) = arg {
            if let syn::Type::Reference(r) = pat.ty.as_ref() {
                return r.mutability.is_some();
            }
        }
        false
    })
}

/// Recursively extract every leaf type-name identifier from a `syn::Type`.
///
/// For `Vec<Result<Account, Error>>` returns `["Vec", "Result", "Account", "Error"]`.
fn extract_type_names(ty: &syn::Type) -> Vec<String> {
    let mut out = Vec::new();
    walk_type(ty, &mut out);
    out
}

fn walk_type(ty: &syn::Type, out: &mut Vec<String>) {
    match ty {
        syn::Type::Path(tp) => {
            for seg in &tp.path.segments {
                out.push(seg.ident.to_string());
                if let syn::PathArguments::AngleBracketed(args) = &seg.arguments {
                    for a in &args.args {
                        if let syn::GenericArgument::Type(inner) = a {
                            walk_type(inner, out);
                        }
                    }
                }
            }
        }
        syn::Type::Reference(r) => walk_type(&r.elem, out),
        syn::Type::Tuple(t) => {
            for inner in &t.elems {
                walk_type(inner, out);
            }
        }
        syn::Type::Array(a) => walk_type(&a.elem, out),
        syn::Type::Slice(s) => walk_type(&s.elem, out),
        syn::Type::Paren(p) => walk_type(&p.elem, out),
        _ => {}
    }
}

/// Whitelist for primitives, std container types, and the `()` unit.
///
/// Container types are recursed into by `extract_type_names` — when this
/// helper says `Vec` is whitelisted, it means the container itself is fine;
/// the inner type (whatever was inside the angle brackets) is checked
/// separately.
fn is_primitive_or_std(name: &str) -> bool {
    matches!(
        name,
        "u8" | "u16" | "u32" | "u64" | "u128" | "usize"
            | "i8" | "i16" | "i32" | "i64" | "i128" | "isize"
            | "f32" | "f64"
            | "bool" | "char" | "str" | "String" | "&str"
            | "Vec" | "Option" | "Result" | "Box" | "Rc" | "Arc" | "Cow"
            | "HashMap" | "HashSet" | "BTreeMap" | "BTreeSet"
            | "VecDeque" | "LinkedList" | "BinaryHeap"
            | "PathBuf" | "Path"
            | "Duration" | "Instant" | "SystemTime"
            | "DateTime" | "Utc" | "NaiveDate" | "NaiveDateTime"
            | "Regex" | "Decimal"
            | "Self"
    )
}

fn kind_label(kind: ArtifactKind) -> &'static str {
    match kind {
        ArtifactKind::Data => "data type",
        ArtifactKind::Projection => "projection",
        ArtifactKind::Context => "context",
        ArtifactKind::Operation => "operation",
        ArtifactKind::Component => "component",
        ArtifactKind::Brand => "brand",
        ArtifactKind::RoutingTable => "routing table",
        ArtifactKind::App => "app",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prepared::{AttributeSpec, FunctionalitySpec, RoleMethodGroupSpec, RoleMethodSpec, RolePlayerSpec};

    fn data_artifact(name: &str, attrs: &[&str]) -> PreparedArtifact {
        let mut a = PreparedArtifact::new(
            ArtifactKind::Data,
            format!("data/{}.md", name),
            name.to_string(),
            name.to_string(),
        );
        a.attributes = attrs
            .iter()
            .map(|n| AttributeSpec {
                name: n.to_string(),
                description: format!("the {}", n),
            })
            .collect();
        a
    }

    fn projection_artifact(
        name: &str,
        role_player: (&str, &str),
        functionalities: &[&str],
    ) -> PreparedArtifact {
        let mut a = PreparedArtifact::new(
            ArtifactKind::Projection,
            format!("projections/{}.md", name),
            name.to_string(),
            name.to_string(),
        );
        a.role_players = vec![RolePlayerSpec {
            name: role_player.0.to_string(),
            data_type: role_player.1.to_string(),
            responsibility: "provides data".to_string(),
        }];
        a.role_methods = vec![RoleMethodGroupSpec {
            role: role_player.0.to_string(),
            methods: vec![RoleMethodSpec {
                name: "read".to_string(),
                canonical_name: format!("{}_read", role_player.0),
                signature_hint: None,
                citation: "provides the data".to_string(),
            }],
        }];
        a.functionalities = functionalities
            .iter()
            .map(|n| FunctionalitySpec {
                name: n.to_string(),
                description: format!("computes {}", n),
                role_method_refs: vec![],
            })
            .collect();
        a
    }

    fn parse(src: &str) -> syn::File {
        syn::parse_str(src).expect("parse")
    }

    // ── Data: check 4 (field visibility) ─────────────────────────

    #[test]
    fn data_pub_field_is_flagged() {
        let src = r#"
            pub struct AuditEntry { pub id: String }
            impl AuditEntry {
                pub fn new(id: String) -> anyhow::Result<Self> { todo!() }
                pub fn id(&self) -> &String { &self.id }
            }
        "#;
        let artifact = data_artifact("AuditEntry", &["id"]);
        let findings = check(&parse(src), &artifact, &KindIndex::empty());
        assert!(findings.iter().any(|d| d.message ==
            "the data type `AuditEntry` declares fields with pub visibility (id) — data fields must be private to preserve the immutability contract."
        ), "got: {:?}", findings);
    }

    // ── Data: check 5 (constructor presence) ─────────────────────

    #[test]
    fn data_missing_new_is_flagged() {
        let src = r#"
            pub struct Money { amount: i64 }
            impl Money {
                pub fn amount(&self) -> i64 { self.amount }
            }
        "#;
        let artifact = data_artifact("Money", &["amount"]);
        let findings = check(&parse(src), &artifact, &KindIndex::empty());
        assert!(findings.iter().any(|d| d.message ==
            "the data type `Money` is missing its required `new()` constructor — every data type must have a constructor that enforces declared invariants."
        ), "got: {:?}", findings);
    }

    // ── Data: check 6 (getter presence) ──────────────────────────

    #[test]
    fn data_missing_getter_is_flagged() {
        let src = r#"
            pub struct Account { iban: String }
            impl Account {
                pub fn new(iban: String) -> anyhow::Result<Self> { todo!() }
            }
        "#;
        let artifact = data_artifact("Account", &["iban"]);
        let findings = check(&parse(src), &artifact, &KindIndex::empty());
        assert!(findings.iter().any(|d| d.message ==
            "the data type `Account` declares an `iban` attribute but the impl block has no `iban()` getter — every declared field must have a getter."
        ), "got: {:?}", findings);
    }

    // ── Data: check 7 (field-type containment) ───────────────────

    #[test]
    fn data_with_projection_field_is_flagged() {
        let src = r#"
            pub struct Account { current_balance: AccountBalance }
            impl Account {
                pub fn new(current_balance: AccountBalance) -> anyhow::Result<Self> { todo!() }
                pub fn current_balance(&self) -> &AccountBalance { &self.current_balance }
            }
        "#;
        let artifact = data_artifact("Account", &["current_balance"]);
        let mut index = KindIndex::empty();
        index.insert("AccountBalance", ArtifactKind::Projection);
        let findings = check(&parse(src), &artifact, &index);
        assert!(findings.iter().any(|d| d.message ==
            "the data type `Account` has a field `current_balance: AccountBalance`, but `AccountBalance` is a projection — data types may only reference other data types."
        ), "got: {:?}", findings);
    }

    // ── Projection: check 8 (field visibility) ───────────────────

    #[test]
    fn projection_pub_field_is_flagged() {
        let src = r#"
            pub struct TransactionList { pub source: Vec<Transaction> }
            impl TransactionList {
                pub fn new(source: Vec<Transaction>) -> Self { todo!() }
            }
        "#;
        let artifact = projection_artifact("TransactionList", ("source", "Transaction"), &[]);
        let findings = check(&parse(src), &artifact, &KindIndex::empty());
        assert!(findings.iter().any(|d| d.message ==
            "the projection `TransactionList` declares fields with pub visibility (source) — projection fields must be private."
        ), "got: {:?}", findings);
    }

    // ── Projection: check 9 (role method visibility) ─────────────

    #[test]
    fn projection_pub_role_method_is_flagged() {
        let src = r#"
            pub struct AccountBalanceSummary { account: Account }
            impl AccountBalanceSummary {
                pub fn new(account: Account) -> Self { todo!() }
                pub fn account_read(&self, account: &Account) {}
            }
        "#;
        let artifact = projection_artifact("AccountBalanceSummary", ("account", "Account"), &[]);
        let findings = check(&parse(src), &artifact, &KindIndex::empty());
        assert!(findings.iter().any(|d| d.message ==
            "the projection `AccountBalanceSummary` declares role method `account_read` as `pub` — projection role methods must be private."
        ), "got: {:?}", findings);
    }

    // ── Projection: check 10 (public surface = functionalities) ──

    #[test]
    fn projection_undeclared_pub_method_is_flagged() {
        let src = r#"
            pub struct AccountBalanceSummary { account: Account }
            impl AccountBalanceSummary {
                pub fn new(account: Account) -> Self { todo!() }
                pub fn format_european(amount: i64) -> String { todo!() }
            }
        "#;
        let artifact = projection_artifact(
            "AccountBalanceSummary",
            ("account", "Account"),
            &["summary"],
        );
        let findings = check(&parse(src), &artifact, &KindIndex::empty());
        assert!(findings.iter().any(|d| d.message ==
            "the projection `AccountBalanceSummary` exposes `format_european` as a public method, but it is not a declared functionality — only declared functionalities may be public on projections."
        ), "got: {:?}", findings);
    }

    // ── Projection: check 11 (immutability) ──────────────────────

    #[test]
    fn projection_mut_self_functionality_is_flagged() {
        let src = r#"
            pub struct TransferSummary { request: TransferRequest }
            impl TransferSummary {
                pub fn new(request: TransferRequest) -> Self { todo!() }
                pub fn summary(&mut self) -> String { todo!() }
            }
        "#;
        let artifact = projection_artifact(
            "TransferSummary",
            ("request", "TransferRequest"),
            &["summary"],
        );
        let findings = check(&parse(src), &artifact, &KindIndex::empty());
        assert!(findings.iter().any(|d| d.message ==
            "the projection `TransferSummary` declares functionality `summary` with `&mut self` — projection functionalities must take `&self`."
        ), "got: {:?}", findings);
    }

    #[test]
    fn projection_role_method_with_mut_role_player_is_flagged() {
        let src = r#"
            pub struct TransactionList { transactions: Transaction }
            impl TransactionList {
                pub fn new(transactions: Transaction) -> Self { todo!() }
                fn transactions_read(&self, transactions: &mut Transaction) {}
            }
        "#;
        let artifact = projection_artifact(
            "TransactionList",
            ("transactions", "Transaction"),
            &[],
        );
        let findings = check(&parse(src), &artifact, &KindIndex::empty());
        assert!(findings.iter().any(|d| d.message ==
            "the projection `TransactionList` declares role method `transactions_read` with the role player as `&mut` — projection role methods must not mutate role players."
        ), "got: {:?}", findings);
    }

    // ── Projection: check 12 (field-type containment) ────────────

    #[test]
    fn projection_with_context_field_is_flagged() {
        let src = r#"
            pub struct TransferSummary { request: TransferDomestic }
            impl TransferSummary {
                pub fn new(request: TransferDomestic) -> Self { todo!() }
            }
        "#;
        let artifact = projection_artifact(
            "TransferSummary",
            ("request", "TransferDomestic"),
            &[],
        );
        let mut index = KindIndex::empty();
        index.insert("TransferDomestic", ArtifactKind::Context);
        let findings = check(&parse(src), &artifact, &index);
        assert!(findings.iter().any(|d| d.message ==
            "the projection `TransferSummary` has a field `request: TransferDomestic`, but `TransferDomestic` is a context — projections may only reference data and other projections."
        ), "got: {:?}", findings);
    }
}
