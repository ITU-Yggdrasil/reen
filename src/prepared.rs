use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const PREPARED_SCHEMA_VERSION: &str = "reen.prepare/v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PreparedArtifact {
    pub schema: String,
    pub source: SourceInfo,
    pub export: ExportInfo,
    /// Whether the enclosing Rust type is mutable (`true` for contexts, `false` for data/projections).
    ///
    /// Mutable types expose their public methods with `&mut self`; immutable types use `&self`.
    /// Role methods always use `&self` on the context regardless of this flag (the role player
    /// argument carries any needed mutability instead).
    ///
    /// Defaults to `false` when absent (legacy prepared files pre-date this field).
    #[serde(default)]
    pub mutable: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<FieldSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variants: Vec<VariantSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roles: Vec<RoleSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub props: Vec<PropSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub collaborators: Vec<CollaboratorSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub functionalities: Vec<MethodSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub getters: Vec<GetterSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub constructor: Option<ConstructorPolicy>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub invariants: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub derives: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ambiguities: Vec<Ambiguity>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceInfo {
    pub path: String,
    pub kind: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExportInfo {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Evidence {
    pub section: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ValueStatus {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rust: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<Evidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FieldSpec {
    pub name: String,
    pub meaning: String,
    #[serde(rename = "type")]
    pub type_status: ValueStatus,
    #[serde(default)]
    pub getter_accessible: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VariantSpec {
    pub name: String,
    pub meaning: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RoleSpec {
    pub name: String,
    pub purpose: String,
    pub expected_behavior: String,
    #[serde(rename = "type")]
    pub type_status: ValueStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub methods: Vec<MethodSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PropSpec {
    pub name: String,
    pub meaning: String,
    #[serde(rename = "type")]
    pub type_status: ValueStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CollaboratorSpec {
    pub name: String,
    pub responsibility: String,
    #[serde(rename = "type")]
    pub type_status: ValueStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GetterSpec {
    pub name: String,
    pub field: String,
    pub mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConstructorPolicy {
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MethodSpec {
    pub name: String,
    pub signature: ValueStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receiver: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parameters: Vec<ParameterSpec>,
    #[serde(rename = "returns")]
    pub return_status: ValueStatus,
    /// Numbered happy-path steps from the draft's **Flow:** section, stripped of their `N. ` prefix.
    /// Present when the flow could not be reduced to machine-readable IR (see `body`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flow: Vec<String>,
    /// Alternative-path entries from the draft's **Extensions:** section (e.g. `1a. …`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extensions: Vec<String>,
    /// Post-condition invariants from the draft's **Guarantee:** section.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub guarantee: Vec<String>,
    /// Identifiers referenced in flow/extensions/guarantee, classified by kind.
    /// Populated when `flow` is non-empty to give an implementation agent a bounded dependency list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub references: Option<MethodReferences>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<Body>,
}

/// Identifiers referenced in a method's behavioral description, classified by their kind in the
/// DCI vocabulary of the owning draft.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct MethodReferences {
    /// Role field names (e.g. `board`, `stdin_source`) mentioned in the flow text.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roles: Vec<String>,
    /// Prop names (e.g. `max_length`) mentioned in the flow text.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub props: Vec<String>,
    /// Type names (PascalCase identifiers, e.g. `KeyPress`, `Direction`) mentioned in the flow text.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub types: Vec<String>,
    /// Role method names as they appear on the context (`<role>_<method>`, e.g. `board_at`,
    /// `stdin_source_read_keys`) inferred from backtick identifiers that match a known role prefix.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub role_methods: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParameterSpec {
    pub name: String,
    #[serde(rename = "type")]
    pub type_status: ValueStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Body {
    pub steps: Vec<Statement>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Statement {
    Let {
        name: String,
        expr: Expression,
    },
    AssignLocal {
        name: String,
        expr: Expression,
    },
    Call {
        expr: Expression,
    },
    If {
        condition: Expression,
        then_steps: Vec<Statement>,
        else_steps: Vec<Statement>,
    },
    Match {
        expr: Expression,
        arms: Vec<MatchArm>,
    },
    ForEach {
        binding: String,
        collection: Expression,
        body: Vec<Statement>,
    },
    Return {
        expr: Option<Expression>,
    },
    SleepMs {
        expr: Expression,
    },
    ReadUtcNowMs {
        name: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MatchArm {
    pub pattern: String,
    pub steps: Vec<Statement>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Expression {
    Literal {
        kind: String,
        value: String,
    },
    Var {
        name: String,
    },
    Field {
        base: String,
        name: String,
    },
    ConstructStruct {
        type_name: String,
        fields: Vec<StructFieldValue>,
    },
    ConstructEnum {
        type_name: String,
        variant: String,
    },
    CallRoleMethod {
        role: String,
        method: String,
        args: Vec<Expression>,
    },
    CallLocalMethod {
        name: String,
        args: Vec<Expression>,
    },
    CallInstanceMethod {
        receiver: Box<Expression>,
        method: String,
        args: Vec<Expression>,
    },
    BinaryOp {
        operator: String,
        left: Box<Expression>,
        right: Box<Expression>,
    },
    UnaryOp {
        operator: String,
        expr: Box<Expression>,
    },
    CollectionLiteral {
        kind: String,
        items: Vec<Expression>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        entries: Vec<CollectionEntry>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StructFieldValue {
    pub name: String,
    pub expr: Expression,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CollectionEntry {
    pub key: Expression,
    pub value: Expression,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Ambiguity {
    pub path: String,
    pub severity: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_line: Option<usize>,
}

impl PreparedArtifact {
    pub fn empty(
        kind: &str,
        path: String,
        title: String,
        export_name: String,
        mutable: bool,
    ) -> Self {
        Self {
            schema: PREPARED_SCHEMA_VERSION.to_string(),
            source: SourceInfo {
                path,
                kind: kind.to_string(),
                title,
            },
            export: ExportInfo { name: export_name },
            mutable,
            fields: Vec::new(),
            variants: Vec::new(),
            roles: Vec::new(),
            props: Vec::new(),
            collaborators: Vec::new(),
            functionalities: Vec::new(),
            getters: Vec::new(),
            constructor: None,
            invariants: Vec::new(),
            derives: Vec::new(),
            ambiguities: Vec::new(),
        }
    }

    pub fn refresh_ambiguity_index(&mut self) {
        let mut ambiguities = Vec::new();
        let mut scanned_paths = BTreeSet::new();
        for (idx, field) in self.fields.iter().enumerate() {
            collect_value_ambiguity(
                &mut ambiguities,
                &mut scanned_paths,
                format!("fields[{idx}].type"),
                &field.type_status,
                &format!("field `{}` type is unresolved", field.name),
            );
        }
        for (idx, role) in self.roles.iter().enumerate() {
            collect_value_ambiguity(
                &mut ambiguities,
                &mut scanned_paths,
                format!("roles[{idx}].type"),
                &role.type_status,
                &format!("role `{}` type is unresolved", role.name),
            );
            for (method_idx, method) in role.methods.iter().enumerate() {
                method.collect_ambiguities(
                    &mut ambiguities,
                    &mut scanned_paths,
                    format!("roles[{idx}].methods[{method_idx}]"),
                );
            }
        }
        for (idx, prop) in self.props.iter().enumerate() {
            collect_value_ambiguity(
                &mut ambiguities,
                &mut scanned_paths,
                format!("props[{idx}].type"),
                &prop.type_status,
                &format!("prop `{}` type is unresolved", prop.name),
            );
        }
        for (idx, collaborator) in self.collaborators.iter().enumerate() {
            collect_value_ambiguity(
                &mut ambiguities,
                &mut scanned_paths,
                format!("collaborators[{idx}].type"),
                &collaborator.type_status,
                &format!("collaborator `{}` type is unresolved", collaborator.name),
            );
        }
        for (idx, method) in self.functionalities.iter().enumerate() {
            method.collect_ambiguities(
                &mut ambiguities,
                &mut scanned_paths,
                format!("functionalities[{idx}]"),
            );
        }
        // Preserve manually-added blocking ambiguities (structural issues from prepare_document)
        // but NOT stale entries for paths the value scan already covers.
        ambiguities.extend(
            self.ambiguities
                .iter()
                .filter(|ambiguity| {
                    ambiguity.severity.eq_ignore_ascii_case("blocking")
                        && !scanned_paths.contains(&ambiguity.path)
                })
                .cloned(),
        );
        ambiguities.sort_by(|left, right| {
            (left.path.as_str(), left.message.as_str())
                .cmp(&(right.path.as_str(), right.message.as_str()))
        });
        ambiguities
            .dedup_by(|left, right| left.path == right.path && left.message == right.message);
        self.ambiguities = ambiguities;
    }

    /// Propagate resolved role/prop types to downstream dependents:
    /// - Role method `<role>_` parameters pick up the role's resolved type.
    /// - Role method signatures are rebuilt to include the role player parameter.
    /// - The auto-constructor signature is rebuilt from current parameter types.
    ///
    /// Idempotent — safe to call after initial prepare and again after fix-agent patches.
    pub fn propagate_resolved_types(&mut self) {
        for role in &mut self.roles {
            let Some(ty) = role.type_status.rust().map(|s| s.to_string()) else {
                continue;
            };
            let role_param_name = format!("{}_", role.name);
            let ref_ty = format!("&{ty}");

            for method in &mut role.methods {
                if let Some(param) = method
                    .parameters
                    .iter_mut()
                    .find(|p| p.name == role_param_name)
                {
                    if !param.type_status.is_resolved() {
                        param.type_status =
                            ValueStatus::resolved(ref_ty.clone(), "prepare.role_player");
                    }
                }

                if let Some(sig) = method.signature.rust().map(|s| s.to_string()) {
                    if !sig.contains(&format!("{role_param_name}:")) {
                        method.signature.rust = Some(insert_role_player_in_signature(
                            &sig,
                            &role_param_name,
                            &ref_ty,
                        ));
                    }
                }
            }
        }

        if let Some(ctor) = self
            .functionalities
            .iter_mut()
            .find(|m| m.name == "new" && m.receiver.is_none())
        {
            let params_text = ctor
                .parameters
                .iter()
                .map(|p| {
                    format!(
                        "{}: {}",
                        p.name,
                        p.type_status.rust.as_deref().unwrap_or("Unknown")
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            let new_sig = format!("new({params_text}) -> Self");
            ctor.signature.rust = Some(new_sig);
        }
    }

    pub fn blocking_ambiguities(&self) -> impl Iterator<Item = &Ambiguity> {
        self.ambiguities
            .iter()
            .filter(|ambiguity| ambiguity.severity.eq_ignore_ascii_case("blocking"))
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema != PREPARED_SCHEMA_VERSION {
            bail!(
                "Unsupported prepared schema `{}` in {}",
                self.schema,
                self.source.path
            );
        }
        validate_methods(&self.functionalities)?;
        for role in &self.roles {
            validate_methods(&role.methods)?;
        }
        Ok(())
    }

    pub fn apply_fix_at_path(&mut self, path: &str, value: &str) -> bool {
        if let Some(vs) = self.resolve_value_status_mut(path) {
            vs.apply_fix(value);
            for amb in &mut self.ambiguities {
                if amb.path == path && amb.severity == "blocking" {
                    amb.severity = "fixed".to_string();
                }
            }
            return true;
        }
        false
    }

    fn resolve_value_status_mut(&mut self, path: &str) -> Option<&mut ValueStatus> {
        if let Some((collection, rest)) = parse_indexed_path(path) {
            match collection {
                "fields" => {
                    let (idx, field) = rest?;
                    let item = self.fields.get_mut(idx)?;
                    match field {
                        "type" => return Some(&mut item.type_status),
                        _ => return None,
                    }
                }
                "roles" => {
                    let (idx, field) = rest?;
                    let role = self.roles.get_mut(idx)?;
                    if field == "type" {
                        return Some(&mut role.type_status);
                    }
                    if let Some(sub) = field.strip_prefix("methods") {
                        if let Some((sub_coll, sub_rest)) =
                            parse_indexed_path(sub.trim_start_matches('.'))
                        {
                            let _ = sub_coll;
                            if let Some((midx, mfield)) = sub_rest {
                                let method = role.methods.get_mut(midx)?;
                                return resolve_method_field(method, mfield);
                            }
                        }
                        let trimmed = sub.trim_start_matches('.');
                        if let Some((sub_coll2, sub_rest2)) = parse_indexed_path(trimmed) {
                            let _ = sub_coll2;
                            if let Some((midx, mfield)) = sub_rest2 {
                                let method = role.methods.get_mut(midx)?;
                                return resolve_method_field(method, mfield);
                            }
                        }
                    }
                }
                "props" => {
                    let (idx, field) = rest?;
                    let item = self.props.get_mut(idx)?;
                    match field {
                        "type" => return Some(&mut item.type_status),
                        _ => return None,
                    }
                }
                "collaborators" => {
                    let (idx, field) = rest?;
                    let item = self.collaborators.get_mut(idx)?;
                    match field {
                        "type" => return Some(&mut item.type_status),
                        _ => return None,
                    }
                }
                "functionalities" => {
                    let (idx, field) = rest?;
                    let method = self.functionalities.get_mut(idx)?;
                    return resolve_method_field(method, field);
                }
                _ => {}
            }
        }
        None
    }

    pub fn referenced_type_names(&self) -> BTreeSet<String> {
        let mut types = BTreeSet::new();
        for field in &self.fields {
            field.type_status.collect_type_names(&mut types);
        }
        for role in &self.roles {
            role.type_status.collect_type_names(&mut types);
            for method in &role.methods {
                method.collect_type_names(&mut types);
            }
        }
        for prop in &self.props {
            prop.type_status.collect_type_names(&mut types);
        }
        for collaborator in &self.collaborators {
            collaborator.type_status.collect_type_names(&mut types);
        }
        for method in &self.functionalities {
            method.collect_type_names(&mut types);
        }
        types.remove(&self.export.name);
        types.remove("Self");
        types
    }
}

impl MethodSpec {
    fn collect_ambiguities(
        &self,
        out: &mut Vec<Ambiguity>,
        scanned: &mut BTreeSet<String>,
        base_path: String,
    ) {
        collect_value_ambiguity(
            out,
            scanned,
            format!("{base_path}.signature"),
            &self.signature,
            &format!("method `{}` signature is unresolved", self.name),
        );
        collect_value_ambiguity(
            out,
            scanned,
            format!("{base_path}.returns"),
            &self.return_status,
            &format!("method `{}` return type is unresolved", self.name),
        );
        for (idx, parameter) in self.parameters.iter().enumerate() {
            collect_value_ambiguity(
                out,
                scanned,
                format!("{base_path}.parameters[{idx}].type"),
                &parameter.type_status,
                &format!("parameter `{}` type is unresolved", parameter.name),
            );
        }
        if self.body.is_none() && self.flow.is_empty() {
            out.push(Ambiguity {
                path: format!("{base_path}.body"),
                severity: "info".to_string(),
                message: format!("method `{}` body is missing", self.name),
                source_line: None,
            });
        }
    }

    fn collect_type_names(&self, types: &mut BTreeSet<String>) {
        self.signature.collect_type_names(types);
        self.return_status.collect_type_names(types);
        for parameter in &self.parameters {
            parameter.type_status.collect_type_names(types);
        }
        if let Some(body) = &self.body {
            body.collect_type_names(types);
        }
    }
}

impl Body {
    fn collect_type_names(&self, types: &mut BTreeSet<String>) {
        for step in &self.steps {
            step.collect_type_names(types);
        }
    }
}

impl Statement {
    fn collect_type_names(&self, types: &mut BTreeSet<String>) {
        match self {
            Statement::Let { expr, .. }
            | Statement::AssignLocal { expr, .. }
            | Statement::Call { expr }
            | Statement::SleepMs { expr } => expr.collect_type_names(types),
            Statement::If {
                condition,
                then_steps,
                else_steps,
            } => {
                condition.collect_type_names(types);
                for step in then_steps {
                    step.collect_type_names(types);
                }
                for step in else_steps {
                    step.collect_type_names(types);
                }
            }
            Statement::Match { expr, arms } => {
                expr.collect_type_names(types);
                for arm in arms {
                    for step in &arm.steps {
                        step.collect_type_names(types);
                    }
                }
            }
            Statement::ForEach {
                collection, body, ..
            } => {
                collection.collect_type_names(types);
                for step in body {
                    step.collect_type_names(types);
                }
            }
            Statement::Return { expr } => {
                if let Some(expr) = expr {
                    expr.collect_type_names(types);
                }
            }
            Statement::ReadUtcNowMs { .. } => {}
        }
    }
}

impl Expression {
    fn collect_type_names(&self, types: &mut BTreeSet<String>) {
        match self {
            Expression::ConstructStruct { type_name, fields } => {
                types.insert(type_name.clone());
                for field in fields {
                    field.expr.collect_type_names(types);
                }
            }
            Expression::ConstructEnum { type_name, .. } => {
                types.insert(type_name.clone());
            }
            Expression::CallRoleMethod { args, .. } | Expression::CallLocalMethod { args, .. } => {
                for arg in args {
                    arg.collect_type_names(types);
                }
            }
            Expression::CallInstanceMethod { receiver, args, .. } => {
                receiver.collect_type_names(types);
                for arg in args {
                    arg.collect_type_names(types);
                }
            }
            Expression::BinaryOp { left, right, .. } => {
                left.collect_type_names(types);
                right.collect_type_names(types);
            }
            Expression::UnaryOp { expr, .. } => expr.collect_type_names(types),
            Expression::CollectionLiteral { items, entries, .. } => {
                for item in items {
                    item.collect_type_names(types);
                }
                for entry in entries {
                    entry.key.collect_type_names(types);
                    entry.value.collect_type_names(types);
                }
            }
            Expression::Literal { .. } | Expression::Var { .. } | Expression::Field { .. } => {}
        }
    }
}

impl ValueStatus {
    pub fn resolved(rust: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            status: "resolved".to_string(),
            rust: Some(rust.into()),
            source: Some(source.into()),
            candidates: Vec::new(),
            reason: None,
            evidence: Vec::new(),
        }
    }

    pub fn defaulted(rust: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            status: "defaulted".to_string(),
            rust: Some(rust.into()),
            source: Some(source.into()),
            candidates: Vec::new(),
            reason: None,
            evidence: Vec::new(),
        }
    }

    pub fn ambiguous(
        candidates: Vec<String>,
        reason: impl Into<String>,
        evidence: Vec<Evidence>,
    ) -> Self {
        Self {
            status: "ambiguous".to_string(),
            rust: None,
            source: None,
            candidates,
            reason: Some(reason.into()),
            evidence,
        }
    }

    pub fn missing(reason: impl Into<String>, evidence: Vec<Evidence>) -> Self {
        Self {
            status: "missing".to_string(),
            rust: None,
            source: None,
            candidates: Vec::new(),
            reason: Some(reason.into()),
            evidence,
        }
    }

    pub fn is_resolved(&self) -> bool {
        matches!(self.status.as_str(), "resolved" | "defaulted" | "fixed")
    }

    pub fn apply_fix(&mut self, value: impl Into<String>) {
        self.status = "fixed".to_string();
        self.rust = Some(value.into());
        self.source = Some("fix.agent".to_string());
    }

    pub fn rust(&self) -> Option<&str> {
        self.rust.as_deref()
    }

    fn collect_type_names(&self, types: &mut BTreeSet<String>) {
        if let Some(rust) = &self.rust {
            for name in extract_type_names(rust) {
                types.insert(name);
            }
        }
    }
}

fn collect_value_ambiguity(
    out: &mut Vec<Ambiguity>,
    scanned: &mut BTreeSet<String>,
    path: String,
    value: &ValueStatus,
    default_message: &str,
) {
    scanned.insert(path.clone());
    if value.is_resolved() {
        return;
    }
    let reason = value.reason.as_deref().unwrap_or(default_message);
    out.push(Ambiguity {
        path,
        severity: "blocking".to_string(),
        message: reason.to_string(),
        source_line: None,
    });
}

fn validate_methods(methods: &[MethodSpec]) -> Result<()> {
    for method in methods {
        if method.name.trim().is_empty() {
            bail!("Prepared methods must have a name");
        }
        if !method.signature.is_resolved() && method.body.is_none() {
            continue;
        }
        if let Some(body) = &method.body {
            validate_body(body)?;
        }
    }
    Ok(())
}

fn validate_body(body: &Body) -> Result<()> {
    for step in &body.steps {
        validate_statement(step)?;
    }
    Ok(())
}

fn validate_statement(step: &Statement) -> Result<()> {
    match step {
        Statement::Let { expr, .. }
        | Statement::AssignLocal { expr, .. }
        | Statement::Call { expr }
        | Statement::SleepMs { expr } => validate_expression(expr),
        Statement::If {
            condition,
            then_steps,
            else_steps,
        } => {
            validate_expression(condition)?;
            for step in then_steps {
                validate_statement(step)?;
            }
            for step in else_steps {
                validate_statement(step)?;
            }
            Ok(())
        }
        Statement::Match { expr, arms } => {
            validate_expression(expr)?;
            for arm in arms {
                for step in &arm.steps {
                    validate_statement(step)?;
                }
            }
            Ok(())
        }
        Statement::ForEach {
            collection, body, ..
        } => {
            validate_expression(collection)?;
            for step in body {
                validate_statement(step)?;
            }
            Ok(())
        }
        Statement::Return { expr } => {
            if let Some(expr) = expr {
                validate_expression(expr)?;
            }
            Ok(())
        }
        Statement::ReadUtcNowMs { .. } => Ok(()),
    }
}

fn validate_expression(expr: &Expression) -> Result<()> {
    match expr {
        Expression::Literal { kind, .. } => match kind.as_str() {
            "string" | "integer" | "bool" | "char" | "path" => Ok(()),
            other => bail!("Unsupported literal kind `{other}`"),
        },
        Expression::Var { .. } | Expression::Field { .. } | Expression::ConstructEnum { .. } => {
            Ok(())
        }
        Expression::ConstructStruct { fields, .. } => {
            for field in fields {
                validate_expression(&field.expr)?;
            }
            Ok(())
        }
        Expression::CallRoleMethod { args, .. } | Expression::CallLocalMethod { args, .. } => {
            for arg in args {
                validate_expression(arg)?;
            }
            Ok(())
        }
        Expression::CallInstanceMethod { receiver, args, .. } => {
            validate_expression(receiver)?;
            for arg in args {
                validate_expression(arg)?;
            }
            Ok(())
        }
        Expression::BinaryOp { left, right, .. } => {
            validate_expression(left)?;
            validate_expression(right)
        }
        Expression::UnaryOp { expr, .. } => validate_expression(expr),
        Expression::CollectionLiteral { items, entries, .. } => {
            for item in items {
                validate_expression(item)?;
            }
            for entry in entries {
                validate_expression(&entry.key)?;
                validate_expression(&entry.value)?;
            }
            Ok(())
        }
    }
}

fn extract_type_names(rust: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut current = String::new();
    let mut prev_sep = '\0';
    for ch in rust.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            current.push(ch);
            continue;
        }
        // Skip names that follow `::` — they are part of a qualified path
        // (e.g. `std::collections::VecDeque`) and not project-local types.
        if prev_sep != ':' {
            push_type_name(&mut names, &mut current);
        } else {
            current.clear();
        }
        prev_sep = ch;
    }
    if prev_sep != ':' {
        push_type_name(&mut names, &mut current);
    } else {
        current.clear();
    }
    names
}

fn push_type_name(names: &mut BTreeSet<String>, current: &mut String) {
    if current.is_empty() {
        return;
    }
    if current
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_uppercase())
    {
        let excluded = [
            "String", "Vec", "Option", "Result", "HashMap", "BTreeMap", "HashSet", "BTreeSet",
            "VecDeque", "Box", "Rc", "Arc", "Cow", "Pin", "Self", "None", "Some", "Ok", "Err",
        ];
        if !excluded.contains(&current.as_str()) {
            names.insert(current.clone());
        }
    }
    current.clear();
}

fn resolve_method_field<'a>(
    method: &'a mut MethodSpec,
    field: &str,
) -> Option<&'a mut ValueStatus> {
    match field {
        "signature" => Some(&mut method.signature),
        "returns" => Some(&mut method.return_status),
        _ => {
            if let Some(rest) = field.strip_prefix("parameters") {
                let rest = rest.trim_start_matches('.');
                if let Some((_, inner)) = parse_indexed_path(rest) {
                    let (pidx, pfield) = inner?;
                    let param = method.parameters.get_mut(pidx)?;
                    if pfield == "type" {
                        return Some(&mut param.type_status);
                    }
                }
            }
            None
        }
    }
}

fn insert_role_player_in_signature(sig: &str, param_name: &str, param_type: &str) -> String {
    let Some(open) = sig.find('(') else {
        return sig.to_string();
    };
    let Some(close) = sig.rfind(')') else {
        return sig.to_string();
    };
    let prefix = &sig[..open + 1];
    let inner = sig[open + 1..close].trim();
    let suffix = &sig[close..];
    let role_param = format!("{param_name}: {param_type}");

    if inner.is_empty() {
        return format!("{prefix}{role_param}{suffix}");
    }

    let first_comma = inner.find(',');
    let first_part = if let Some(pos) = first_comma {
        inner[..pos].trim()
    } else {
        inner
    };
    let is_receiver = matches!(first_part, "&self" | "&mut self" | "self");

    if is_receiver {
        if let Some(pos) = first_comma {
            let rest = inner[pos + 1..].trim();
            format!("{prefix}{first_part}, {role_param}, {rest}{suffix}")
        } else {
            format!("{prefix}{first_part}, {role_param}{suffix}")
        }
    } else {
        format!("{prefix}{role_param}, {inner}{suffix}")
    }
}

fn parse_indexed_path(path: &str) -> Option<(&str, Option<(usize, &str)>)> {
    let open = path.find('[')?;
    let close = path.find(']')?;
    let collection = &path[..open];
    let idx: usize = path[open + 1..close].parse().ok()?;
    let rest = path[close + 1..].trim_start_matches('.');
    if rest.is_empty() {
        Some((collection, Some((idx, ""))))
    } else {
        Some((collection, Some((idx, rest))))
    }
}
