//! Structural-drift checks (verify hard gates 13–17).
//!
//! These complement the kind-discipline gates in `verify_kind` and the
//! existing structural pass in `verify`. The drift gates catch the
//! deterministic subset of audit findings — canonical role-method-name
//! mismatches, undeclared struct fields, role-method bypass, dead role
//! methods, missing-audit-on-failure — so the LLM-backed auditor can
//! focus on semantic judgement rather than mechanical comparison.
//!
//! All checks parse Rust with `syn` (no regex), use DCI-language messages,
//! and have locked-wording unit tests.

use crate::prepared::{ArtifactKind, PreparedArtifact};
use crate::verify::{Discrepancy, DiscrepancyKind};
use std::collections::BTreeSet;

/// Run the drift checks against a parsed Rust file.
pub fn check(file: &syn::File, artifact: &PreparedArtifact) -> Vec<Discrepancy> {
    let mut findings = Vec::new();
    match artifact.kind {
        ArtifactKind::Context | ArtifactKind::Projection => {
            check_canonical_role_method_names(file, artifact, &mut findings); // 13
            check_struct_field_alignment(file, artifact, &mut findings); // 14
            check_role_method_bypass(file, artifact, &mut findings); // 15
            check_dead_role_methods(file, artifact, &mut findings); // 16
            check_audit_emission_on_failure(file, artifact, &mut findings); // 17 (warning)
        }
        ArtifactKind::Data => {
            check_struct_field_alignment(file, artifact, &mut findings); // 14
        }
        _ => {}
    }
    findings
}

// ── Check 13: canonical role method name alignment ───────────────

fn check_canonical_role_method_names(
    file: &syn::File,
    artifact: &PreparedArtifact,
    findings: &mut Vec<Discrepancy>,
) {
    let target = &artifact.export.name;
    let methods = collect_methods(file, target);

    let canonical_names: BTreeSet<String> = artifact
        .role_methods
        .iter()
        .flat_map(|g| g.methods.iter().map(|m| m.canonical_name.clone()))
        .collect();

    let kind_label = artefact_label(&artifact.kind);
    for canonical in &canonical_names {
        let exists = methods.iter().any(|(name, _)| name == canonical);
        if !exists {
            findings.push(Discrepancy {
                kind: DiscrepancyKind::MissingMethod,
                message: format!(
                    "the {} `{}` declares role method `{}` in its prepared YAML but the implementation does not define it — role method names are contractual.",
                    kind_label,
                    target,
                    canonical,
                ),
            });
        }
    }
}

// ── Check 14: struct field declaration alignment ─────────────────

fn check_struct_field_alignment(
    file: &syn::File,
    artifact: &PreparedArtifact,
    findings: &mut Vec<Discrepancy>,
) {
    let target = &artifact.export.name;
    let Some(struct_item) = find_struct(file, target) else {
        return;
    };

    let expected: BTreeSet<String> = expected_struct_fields(artifact);
    let actual: Vec<String> = struct_item
        .fields
        .iter()
        .filter_map(|f| f.ident.as_ref().map(|i| i.to_string()))
        .collect();

    let mut undeclared: Vec<String> = Vec::new();
    for name in &actual {
        if expected.contains(name) {
            continue;
        }
        if name.starts_with("internal_") {
            continue;
        }
        undeclared.push(name.clone());
    }

    if !undeclared.is_empty() {
        let kind_label = artefact_label(&artifact.kind);
        findings.push(Discrepancy {
            kind: DiscrepancyKind::ExtraField,
            message: format!(
                "the {} `{}` declares fields ({}) that are not in its prepared YAML's role players, props, attributes, or source — bookkeeping fields are permitted but must be prefixed `internal_`.",
                kind_label,
                target,
                undeclared.join(", "),
            ),
        });
    }
}

fn expected_struct_fields(artifact: &PreparedArtifact) -> BTreeSet<String> {
    let mut fields = BTreeSet::new();
    match artifact.kind {
        ArtifactKind::Context | ArtifactKind::Projection => {
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
        _ => {}
    }
    fields
}

// ── Check 15: role-method-bypass detection ───────────────────────

fn check_role_method_bypass(
    file: &syn::File,
    artifact: &PreparedArtifact,
    findings: &mut Vec<Discrepancy>,
) {
    let target = &artifact.export.name;
    let canonical_names: BTreeSet<String> = artifact
        .role_methods
        .iter()
        .flat_map(|g| g.methods.iter().map(|m| m.canonical_name.clone()))
        .collect();
    if canonical_names.is_empty() {
        return;
    }

    let methods = collect_methods(file, target);
    let mut bypasses: Vec<(String, String, String)> = Vec::new();
    for (method_name, fn_item) in &methods {
        // Walk this method's body looking for `self.<field>.<method>(...)`
        // calls whose `<field>_<method>` matches a canonical role-method name.
        let mut calls: Vec<(String, String)> = Vec::new();
        collect_self_field_calls(&fn_item.block, &mut calls);
        for (field, called) in calls {
            let canonical = format!("{}_{}", field, called);
            if canonical_names.contains(&canonical) {
                bypasses.push((method_name.clone(), field, called));
            }
        }
    }

    let kind_label = artefact_label(&artifact.kind);
    for (caller, field, called) in bypasses {
        findings.push(Discrepancy {
            kind: DiscrepancyKind::WrongVisibility,
            message: format!(
                "the {} `{}` calls `self.{}.{}` directly inside `{}`, bypassing the declared role method `{}_{}`.",
                kind_label,
                target,
                field,
                called,
                caller,
                field,
                called,
            ),
        });
    }
}

// ── Check 16: dead role method detection ─────────────────────────

fn check_dead_role_methods(
    file: &syn::File,
    artifact: &PreparedArtifact,
    findings: &mut Vec<Discrepancy>,
) {
    let target = &artifact.export.name;
    let canonical_names: BTreeSet<String> = artifact
        .role_methods
        .iter()
        .flat_map(|g| g.methods.iter().map(|m| m.canonical_name.clone()))
        .collect();
    if canonical_names.is_empty() {
        return;
    }

    let methods = collect_methods(file, target);

    // Skip the dead-role-method check when the file is freshly scaffolded
    // (every method body is just `todo!()`). The check fires only after at
    // least one functionality body has been filled in — otherwise it would
    // false-positive on every new scaffold.
    let fresh_scaffold = methods
        .iter()
        .all(|(_, fn_item)| body_is_todo(&fn_item.block));
    if fresh_scaffold {
        return;
    }

    let mut all_self_calls: BTreeSet<String> = BTreeSet::new();
    for (_, fn_item) in &methods {
        collect_self_method_calls(&fn_item.block, &mut all_self_calls);
    }

    let kind_label = artefact_label(&artifact.kind);
    for canonical in &canonical_names {
        if !all_self_calls.contains(canonical) {
            findings.push(Discrepancy {
                kind: DiscrepancyKind::ExtraMethod,
                message: format!(
                    "the role method `{}` is declared on the {} `{}` but never invoked from any functionality body or other role method.",
                    canonical,
                    kind_label,
                    target,
                ),
            });
        }
    }
}

/// True when the block contains only a `todo!()` statement (with optional
/// argument), modulo whitespace and braces. Used to detect freshly-scaffolded
/// methods that haven't been implemented yet.
fn body_is_todo(block: &syn::Block) -> bool {
    if block.stmts.len() != 1 {
        return false;
    }
    let stmt = &block.stmts[0];
    let expr = match stmt {
        syn::Stmt::Expr(e, _) => e,
        syn::Stmt::Macro(m) => {
            return m
                .mac
                .path
                .segments
                .last()
                .map(|s| s.ident == "todo")
                .unwrap_or(false);
        }
        _ => return false,
    };
    if let syn::Expr::Macro(m) = expr {
        return m
            .mac
            .path
            .segments
            .last()
            .map(|s| s.ident == "todo")
            .unwrap_or(false);
    }
    false
}

// ── Check 17: audit emission on failure scenarios (warning) ──────

fn check_audit_emission_on_failure(
    file: &syn::File,
    artifact: &PreparedArtifact,
    findings: &mut Vec<Discrepancy>,
) {
    let target = &artifact.export.name;

    // Find functionalities whose scenarios mention failure-shaped audit
    // outcomes. Heuristic match on the scenario then-clause.
    let failure_keywords = ["denied", "failed", "rejected", "denial", "failure"];
    let mut functionalities_with_failure_scenarios: BTreeSet<String> = BTreeSet::new();
    for func in &artifact.functionalities {
        for scenario in &artifact.scenarios {
            // Naive correlation: if the scenario name or `then` text mentions
            // an audit + failure keyword, attribute it to all functionalities
            // for now. (The prepared YAML doesn't carry an explicit
            // scenario→functionality binding for v3; refine when it does.)
            let blob = format!("{} {}", scenario.name, scenario.then).to_ascii_lowercase();
            if blob.contains("audit") && failure_keywords.iter().any(|kw| blob.contains(kw)) {
                functionalities_with_failure_scenarios.insert(func.name.clone());
            }
        }
    }
    if functionalities_with_failure_scenarios.is_empty() {
        return;
    }

    let methods = collect_methods(file, target);
    let kind_label = artefact_label(&artifact.kind);
    for func_name in &functionalities_with_failure_scenarios {
        let snake = crate::codegen::to_snake_case(func_name);
        let Some((_, fn_item)) = methods.iter().find(|(n, _)| n == &snake) else {
            continue;
        };
        // Body contains an early-return-on-error path?
        let mut has_failure_path = false;
        let mut has_audit_call = false;
        scan_body(&fn_item.block, &mut has_failure_path, &mut has_audit_call);
        if has_failure_path && !has_audit_call {
            findings.push(Discrepancy {
                kind: DiscrepancyKind::MissingMethod,
                message: format!(
                    "(warning) the functionality `{}` on {} `{}` has scenarios describing audit-on-failure outcomes, but its body has at least one error-returning path with no audit-emitting call before the return.",
                    func_name,
                    kind_label,
                    target,
                ),
            });
        }
    }
}

fn scan_body(block: &syn::Block, has_failure_path: &mut bool, has_audit_call: &mut bool) {
    use syn::visit::Visit;

    struct V<'a> {
        has_failure_path: &'a mut bool,
        has_audit_call: &'a mut bool,
    }

    impl<'ast> Visit<'ast> for V<'_> {
        fn visit_expr(&mut self, expr: &'ast syn::Expr) {
            match expr {
                syn::Expr::Try(_) => *self.has_failure_path = true,
                syn::Expr::Return(ret) => {
                    if let Some(inner) = &ret.expr {
                        // return Err(...)
                        if expr_starts_with_ident(inner, "Err") {
                            *self.has_failure_path = true;
                        }
                    }
                }
                syn::Expr::Macro(m) => {
                    let path = m.mac.path.segments.last().map(|s| s.ident.to_string());
                    if matches!(path.as_deref(), Some("bail") | Some("ensure") | Some("anyhow")) {
                        *self.has_failure_path = true;
                    }
                }
                syn::Expr::MethodCall(mc) => {
                    let name = mc.method.to_string();
                    if name.contains("audit") {
                        *self.has_audit_call = true;
                    }
                }
                syn::Expr::Call(call) => {
                    if let syn::Expr::Path(ep) = call.func.as_ref() {
                        if let Some(seg) = ep.path.segments.last() {
                            if seg.ident.to_string().contains("audit") {
                                *self.has_audit_call = true;
                            }
                        }
                    }
                }
                _ => {}
            }
            syn::visit::visit_expr(self, expr);
        }
    }

    let mut v = V { has_failure_path, has_audit_call };
    v.visit_block(block);
}

fn expr_starts_with_ident(expr: &syn::Expr, ident: &str) -> bool {
    match expr {
        syn::Expr::Call(call) => {
            if let syn::Expr::Path(ep) = call.func.as_ref() {
                return ep
                    .path
                    .segments
                    .last()
                    .map(|s| s.ident == ident)
                    .unwrap_or(false);
            }
            false
        }
        syn::Expr::Path(ep) => ep
            .path
            .segments
            .last()
            .map(|s| s.ident == ident)
            .unwrap_or(false),
        _ => false,
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

fn collect_methods<'f>(file: &'f syn::File, target: &str) -> Vec<(String, &'f syn::ImplItemFn)> {
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
                    out.push((method.sig.ident.to_string(), method));
                }
            }
        }
    }
    out
}

/// Collect `self.<field>.<method>(...)` calls inside a block.
fn collect_self_field_calls(block: &syn::Block, out: &mut Vec<(String, String)>) {
    use syn::visit::Visit;
    struct V<'a>(&'a mut Vec<(String, String)>);
    impl<'ast> Visit<'ast> for V<'_> {
        fn visit_expr_method_call(&mut self, mc: &'ast syn::ExprMethodCall) {
            if let syn::Expr::Field(field) = mc.receiver.as_ref() {
                if let syn::Expr::Path(ep) = field.base.as_ref() {
                    if ep.path.is_ident("self") {
                        if let syn::Member::Named(field_ident) = &field.member {
                            self.0.push((field_ident.to_string(), mc.method.to_string()));
                        }
                    }
                }
            }
            syn::visit::visit_expr_method_call(self, mc);
        }
    }
    V(out).visit_block(block);
}

/// Collect `self.<method>(...)` calls inside a block.
fn collect_self_method_calls(block: &syn::Block, out: &mut BTreeSet<String>) {
    use syn::visit::Visit;
    struct V<'a>(&'a mut BTreeSet<String>);
    impl<'ast> Visit<'ast> for V<'_> {
        fn visit_expr_method_call(&mut self, mc: &'ast syn::ExprMethodCall) {
            if let syn::Expr::Path(ep) = mc.receiver.as_ref() {
                if ep.path.is_ident("self") {
                    self.0.insert(mc.method.to_string());
                }
            }
            syn::visit::visit_expr_method_call(self, mc);
        }
    }
    V(out).visit_block(block);
}

fn artefact_label(kind: &ArtifactKind) -> &'static str {
    match kind {
        ArtifactKind::Context => "context",
        ArtifactKind::Data => "data type",
        ArtifactKind::Projection => "projection",
        _ => "artefact",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prepared::{
        FunctionalitySpec, RoleMethodGroupSpec, RoleMethodSpec, RolePlayerSpec, ScenarioSpec,
    };

    fn parse(src: &str) -> syn::File {
        syn::parse_str(src).expect("parse")
    }

    fn context_artifact(name: &str) -> PreparedArtifact {
        PreparedArtifact::new(
            ArtifactKind::Context,
            format!("contexts/{}.md", name),
            name.to_string(),
            name.to_string(),
        )
    }

    // ── Check 13: canonical role method names ───────────────────

    #[test]
    fn canonical_role_method_name_mismatch_is_flagged() {
        let mut artifact = context_artifact("chat_session");
        artifact.role_methods = vec![RoleMethodGroupSpec {
            role: "conversation".into(),
            methods: vec![RoleMethodSpec {
                name: "load_history".into(),
                canonical_name: "conversation_load_history".into(),
                signature_hint: None,
                citation: "retrieves prior turns".into(),
            }],
        }];
        let src = r#"
            pub struct chat_session {}
            impl chat_session {
                fn session_load_conversation(&self) {}
            }
        "#;
        let findings = check(&parse(src), &artifact);
        assert!(findings.iter().any(|d| d.message ==
            "the context `chat_session` declares role method `conversation_load_history` in its prepared YAML but the implementation does not define it — role method names are contractual."
        ), "got: {:?}", findings);
    }

    // ── Check 14: struct field alignment ────────────────────────

    #[test]
    fn undeclared_struct_field_is_flagged() {
        let mut artifact = context_artifact("record_audit_entry");
        artifact.role_players = vec![RolePlayerSpec {
            name: "audit_entry".into(),
            data_type: "AuditEntry".into(),
            responsibility: "the entry".into(),
        }];
        let src = r#"
            pub struct record_audit_entry {
                audit_entry: AuditEntry,
                audit_sink: Vec<AuditEntry>,
                event_descriptor: String,
            }
            impl record_audit_entry {}
        "#;
        let findings = check(&parse(src), &artifact);
        assert!(findings.iter().any(|d| d.message ==
            "the context `record_audit_entry` declares fields (audit_sink, event_descriptor) that are not in its prepared YAML's role players, props, attributes, or source — bookkeeping fields are permitted but must be prefixed `internal_`."
        ), "got: {:?}", findings);
    }

    #[test]
    fn internal_prefixed_field_is_allowed() {
        let mut artifact = context_artifact("session_manager");
        artifact.role_players = vec![RolePlayerSpec {
            name: "session".into(),
            data_type: "Session".into(),
            responsibility: "the session".into(),
        }];
        let src = r#"
            pub struct session_manager {
                session: Session,
                internal_audit_buffer: Vec<()>,
            }
            impl session_manager {}
        "#;
        let findings = check(&parse(src), &artifact);
        assert!(
            !findings.iter().any(|d| matches!(d.kind, DiscrepancyKind::ExtraField)),
            "internal_-prefixed field should be allowed: {:?}", findings
        );
    }

    // ── Check 15: role-method-bypass ────────────────────────────

    #[test]
    fn role_method_bypass_is_flagged() {
        let mut artifact = context_artifact("enforce_idempotency");
        artifact.role_players = vec![RolePlayerSpec {
            name: "idempotency_ledger".into(),
            data_type: "IdempotencyLedger".into(),
            responsibility: "ledger".into(),
        }];
        artifact.role_methods = vec![RoleMethodGroupSpec {
            role: "idempotency_ledger".into(),
            methods: vec![RoleMethodSpec {
                name: "lookup".into(),
                canonical_name: "idempotency_ledger_lookup".into(),
                signature_hint: None,
                citation: "looks up the token".into(),
            }],
        }];
        artifact.functionalities = vec![FunctionalitySpec {
            name: "enforce".into(),
            description: "enforces".into(),
            role_method_refs: vec!["idempotency_ledger_lookup".into()],
        }];
        let src = r#"
            pub struct enforce_idempotency { idempotency_ledger: IdempotencyLedger }
            impl enforce_idempotency {
                pub fn enforce(&self) {
                    let _ = self.idempotency_ledger.lookup();
                }
                fn idempotency_ledger_lookup(&self, _: &IdempotencyLedger) {}
            }
        "#;
        let findings = check(&parse(src), &artifact);
        assert!(findings.iter().any(|d| d.message ==
            "the context `enforce_idempotency` calls `self.idempotency_ledger.lookup` directly inside `enforce`, bypassing the declared role method `idempotency_ledger_lookup`."
        ), "got: {:?}", findings);
    }

    // ── Check 16: dead role method ──────────────────────────────

    #[test]
    fn dead_role_method_is_flagged() {
        let mut artifact = context_artifact("revoke_session");
        artifact.role_players = vec![RolePlayerSpec {
            name: "audit_sink".into(),
            data_type: "AuditSink".into(),
            responsibility: "audit sink".into(),
        }];
        artifact.role_methods = vec![RoleMethodGroupSpec {
            role: "audit_sink".into(),
            methods: vec![RoleMethodSpec {
                name: "record".into(),
                canonical_name: "audit_sink_record".into(),
                signature_hint: None,
                citation: "records".into(),
            }],
        }];
        artifact.functionalities = vec![FunctionalitySpec {
            name: "revoke".into(),
            description: "revoke".into(),
            role_method_refs: vec!["audit_sink_record".into()],
        }];
        let src = r#"
            pub struct revoke_session { audit_sink: AuditSink }
            impl revoke_session {
                pub fn revoke(&self) {}
                fn audit_sink_record(&self, _: &AuditSink) {}
            }
        "#;
        let findings = check(&parse(src), &artifact);
        assert!(findings.iter().any(|d| d.message ==
            "the role method `audit_sink_record` is declared on the context `revoke_session` but never invoked from any functionality body or other role method."
        ), "got: {:?}", findings);
    }

    // ── Check 17: audit emission on failure (warning) ───────────

    #[test]
    fn missing_audit_on_failure_path_is_warned() {
        let mut artifact = context_artifact("list_customer_accounts");
        artifact.functionalities = vec![FunctionalitySpec {
            name: "list".into(),
            description: "lists accounts; refuses on missing scope".into(),
            role_method_refs: vec![],
        }];
        artifact.scenarios = vec![ScenarioSpec {
            name: "scope denied".into(),
            given: "customer without accounts-read scope".into(),
            when: "list is invoked".into(),
            then: "the listing is denied and an audit entry records the denial".into(),
        }];
        let src = r#"
            pub struct list_customer_accounts {}
            impl list_customer_accounts {
                pub fn list(&self) -> anyhow::Result<()> {
                    self.verify_scope()?;
                    Ok(())
                }
                fn verify_scope(&self) -> anyhow::Result<()> { Ok(()) }
            }
        "#;
        let findings = check(&parse(src), &artifact);
        assert!(findings.iter().any(|d| d.message ==
            "(warning) the functionality `list` on context `list_customer_accounts` has scenarios describing audit-on-failure outcomes, but its body has at least one error-returning path with no audit-emitting call before the return."
        ), "got: {:?}", findings);
    }

    #[test]
    fn audit_emission_on_failure_path_is_clean() {
        // Same artefact as the previous test, but the body emits an audit
        // before propagating the error.
        let mut artifact = context_artifact("list_customer_accounts");
        artifact.functionalities = vec![FunctionalitySpec {
            name: "list".into(),
            description: "lists accounts; refuses on missing scope".into(),
            role_method_refs: vec![],
        }];
        artifact.scenarios = vec![ScenarioSpec {
            name: "scope denied".into(),
            given: "customer without accounts-read scope".into(),
            when: "list is invoked".into(),
            then: "the listing is denied and an audit entry records the denial".into(),
        }];
        let src = r#"
            pub struct list_customer_accounts {}
            impl list_customer_accounts {
                pub fn list(&self) -> anyhow::Result<()> {
                    if let Err(e) = self.verify_scope() {
                        self.audit_sink_record_denial();
                        return Err(e);
                    }
                    Ok(())
                }
                fn verify_scope(&self) -> anyhow::Result<()> { Ok(()) }
                fn audit_sink_record_denial(&self) {}
            }
        "#;
        let findings = check(&parse(src), &artifact);
        assert!(
            !findings.iter().any(|d| d.message.contains("(warning)")),
            "audit-on-failure call should suppress the warning: {:?}", findings
        );
    }
}
