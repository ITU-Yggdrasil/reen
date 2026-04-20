use crate::prepared::{
    Ambiguity, Body, CollectionEntry, Expression, GetterSpec, MatchArm, MethodSpec, ParameterSpec,
    PreparedArtifact, RoleSpec, Statement, StructFieldValue,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractIssue {
    pub artifact_index: usize,
    pub ambiguity: Ambiguity,
}

pub fn collect_contract_issues(artifacts: &[PreparedArtifact]) -> Vec<ContractIssue> {
    let index = ArtifactIndex::new(artifacts);
    let mut issues = Vec::new();

    for (artifact_index, artifact) in artifacts.iter().enumerate() {
        validate_collab_sync_wrappers(artifact_index, artifact, &index, &mut issues);
        validate_body_contracts(artifact_index, artifact, &index, &mut issues);
    }

    issues.sort_by(|left, right| {
        (
            left.artifact_index,
            left.ambiguity.path.as_str(),
            left.ambiguity.message.as_str(),
        )
            .cmp(&(
                right.artifact_index,
                right.ambiguity.path.as_str(),
                right.ambiguity.message.as_str(),
            ))
    });
    issues.dedup_by(|left, right| {
        left.artifact_index == right.artifact_index
            && left.ambiguity.path == right.ambiguity.path
            && left.ambiguity.message == right.ambiguity.message
    });
    issues
}

#[derive(Debug)]
struct ArtifactIndex<'a> {
    artifacts: &'a [PreparedArtifact],
    by_export: BTreeMap<String, usize>,
}

impl<'a> ArtifactIndex<'a> {
    fn new(artifacts: &'a [PreparedArtifact]) -> Self {
        let mut by_export = BTreeMap::new();
        for (idx, artifact) in artifacts.iter().enumerate() {
            by_export.insert(artifact.export.name.clone(), idx);
        }
        Self {
            artifacts,
            by_export,
        }
    }

    fn local_artifact(
        &self,
        rust: &str,
        current_export: &str,
    ) -> Option<(usize, &'a PreparedArtifact)> {
        let simple = simple_type_name(&replace_self_type(rust, current_export));
        let idx = self.by_export.get(&simple).copied()?;
        Some((idx, &self.artifacts[idx]))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum InferredType {
    Unknown,
    IntegerLiteral,
    Known(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReceiverKind {
    Ref,
    MutRef,
    Owned,
    None,
}

#[derive(Debug, Clone)]
struct PublicMethodContract {
    receiver: ReceiverKind,
    parameters: Vec<NamedType>,
    return_type: String,
    rendered_signature: String,
}

#[derive(Debug, Clone)]
struct NamedType {
    name: String,
    rust: String,
}

#[derive(Debug, Clone)]
struct FieldContract {
    rust: String,
}

#[derive(Debug)]
struct MethodValidator<'a, 'b> {
    artifact_index: usize,
    artifact: &'a PreparedArtifact,
    index: &'b ArtifactIndex<'a>,
    issues: &'b mut Vec<ContractIssue>,
}

impl<'a, 'b> MethodValidator<'a, 'b> {
    fn validate_method(&mut self, method: &MethodSpec, method_path: &str) {
        let Some(body) = &method.body else {
            return;
        };

        let mut env = self.base_environment();
        env.insert(
            "self".to_string(),
            InferredType::Known(self.artifact.export.name.clone()),
        );
        for parameter in &method.parameters {
            if let Some(rust) = parameter.type_status.rust() {
                env.insert(
                    parameter.name.clone(),
                    InferredType::Known(rust.to_string()),
                );
            }
        }

        self.validate_body(body, method_path, method.return_status.rust(), &mut env);
    }

    fn base_environment(&self) -> BTreeMap<String, InferredType> {
        let mut env = BTreeMap::new();
        for field in &self.artifact.fields {
            if let Some(rust) = field.type_status.rust() {
                env.insert(field.name.clone(), InferredType::Known(rust.to_string()));
            }
        }
        for role in &self.artifact.roles {
            if let Some(rust) = role.type_status.rust() {
                env.insert(role.name.clone(), InferredType::Known(rust.to_string()));
            }
        }
        for prop in &self.artifact.props {
            if let Some(rust) = prop.type_status.rust() {
                env.insert(prop.name.clone(), InferredType::Known(rust.to_string()));
            }
        }
        for collaborator in &self.artifact.collaborators {
            if let Some(rust) = collaborator.type_status.rust() {
                env.insert(
                    collaborator.name.clone(),
                    InferredType::Known(rust.to_string()),
                );
            }
        }
        env
    }

    fn validate_body(
        &mut self,
        body: &Body,
        method_path: &str,
        return_type: Option<&str>,
        env: &mut BTreeMap<String, InferredType>,
    ) {
        for (step_idx, step) in body.steps.iter().enumerate() {
            self.validate_statement(
                step,
                &format!("{method_path}.body.steps[{step_idx}]"),
                return_type,
                env,
            );
        }
    }

    fn validate_statement(
        &mut self,
        step: &Statement,
        path: &str,
        return_type: Option<&str>,
        env: &mut BTreeMap<String, InferredType>,
    ) {
        match step {
            Statement::Let { name, expr } => {
                let ty = self.infer_expression(expr, &format!("{path}.expr"), env);
                env.insert(name.clone(), ty);
            }
            Statement::AssignLocal { name, expr } => {
                let value_type = self.infer_expression(expr, &format!("{path}.expr"), env);
                if let Some(expected) = env.get(name) {
                    if !types_compatible(expected, &value_type, &self.artifact.export.name, true) {
                        self.issue(
                            path,
                            format!(
                                "assignment to `{name}` expects {} but got {}",
                                render_inferred_type(expected),
                                render_inferred_type(&value_type)
                            ),
                        );
                    }
                }
                env.insert(name.clone(), value_type);
            }
            Statement::Call { expr } => {
                self.infer_expression(expr, &format!("{path}.expr"), env);
            }
            Statement::If {
                condition,
                then_steps,
                else_steps,
            } => {
                let condition_type =
                    self.infer_expression(condition, &format!("{path}.condition"), env);
                if !bool_compatible(&condition_type, &self.artifact.export.name) {
                    self.issue(
                        &format!("{path}.condition"),
                        format!(
                            "if condition expects `bool` but got {}",
                            render_inferred_type(&condition_type)
                        ),
                    );
                }
                let mut then_env = env.clone();
                for (idx, step) in then_steps.iter().enumerate() {
                    self.validate_statement(
                        step,
                        &format!("{path}.then_steps[{idx}]"),
                        return_type,
                        &mut then_env,
                    );
                }
                let mut else_env = env.clone();
                for (idx, step) in else_steps.iter().enumerate() {
                    self.validate_statement(
                        step,
                        &format!("{path}.else_steps[{idx}]"),
                        return_type,
                        &mut else_env,
                    );
                }
            }
            Statement::Match { expr, arms } => {
                self.infer_expression(expr, &format!("{path}.expr"), env);
                for (idx, arm) in arms.iter().enumerate() {
                    self.validate_match_arm(
                        arm,
                        &format!("{path}.arms[{idx}]"),
                        return_type,
                        &mut env.clone(),
                    );
                }
            }
            Statement::ForEach {
                binding,
                collection,
                body,
            } => {
                let collection_type =
                    self.infer_expression(collection, &format!("{path}.collection"), env);
                let mut body_env = env.clone();
                if let Some(item_type) =
                    foreach_item_type(&collection_type, &self.artifact.export.name)
                {
                    body_env.insert(binding.clone(), InferredType::Known(item_type));
                }
                for (idx, step) in body.iter().enumerate() {
                    self.validate_statement(
                        step,
                        &format!("{path}.body[{idx}]"),
                        return_type,
                        &mut body_env,
                    );
                }
            }
            Statement::Return { expr } => {
                let actual = match expr {
                    Some(expr) => self.infer_expression(expr, &format!("{path}.expr"), env),
                    None => InferredType::Known("()".to_string()),
                };
                if let Some(expected) = return_type {
                    if !types_compatible_known(expected, &actual, &self.artifact.export.name, true)
                    {
                        self.issue(
                            path,
                            format!(
                                "return type expects `{}` but got {}",
                                replace_self_type(expected, &self.artifact.export.name),
                                render_inferred_type(&actual)
                            ),
                        );
                    }
                }
            }
            Statement::SleepMs { expr } => {
                let actual = self.infer_expression(expr, &format!("{path}.expr"), env);
                if !matches!(actual, InferredType::Unknown | InferredType::IntegerLiteral)
                    && !matches!(actual, InferredType::Known(ref rust) if is_integer_type(rust))
                {
                    self.issue(
                        &format!("{path}.expr"),
                        format!(
                            "sleep duration expects an integer millisecond value but got {}",
                            render_inferred_type(&actual)
                        ),
                    );
                }
            }
            Statement::ReadUtcNowMs { name } => {
                env.insert(name.clone(), InferredType::Known("u64".to_string()));
            }
        }
    }

    fn validate_match_arm(
        &mut self,
        arm: &MatchArm,
        path: &str,
        return_type: Option<&str>,
        env: &mut BTreeMap<String, InferredType>,
    ) {
        for (idx, step) in arm.steps.iter().enumerate() {
            self.validate_statement(step, &format!("{path}.steps[{idx}]"), return_type, env);
        }
    }

    fn infer_expression(
        &mut self,
        expr: &Expression,
        path: &str,
        env: &BTreeMap<String, InferredType>,
    ) -> InferredType {
        match expr {
            Expression::Literal { kind, .. } => match kind.as_str() {
                "string" => InferredType::Known("String".to_string()),
                "bool" => InferredType::Known("bool".to_string()),
                "char" => InferredType::Known("char".to_string()),
                "integer" => InferredType::IntegerLiteral,
                _ => InferredType::Unknown,
            },
            Expression::Var { name } => env.get(name).cloned().unwrap_or(InferredType::Unknown),
            Expression::Field { base, name } => self.infer_field(base, name, path, env),
            Expression::ConstructStruct { type_name, fields } => {
                self.validate_struct_construction(type_name, fields, path, env)
            }
            Expression::ConstructEnum { type_name, .. } => {
                InferredType::Known(replace_self_type(type_name, &self.artifact.export.name))
            }
            Expression::CallRoleMethod { role, method, args } => {
                self.validate_role_method_call(role, method, args, path, env)
            }
            Expression::CallLocalMethod { name, args } => {
                self.validate_local_method_call(name, args, path, env)
            }
            Expression::CallInstanceMethod {
                receiver,
                method,
                args,
            } => self.validate_instance_method_call(receiver, method, args, path, env),
            Expression::BinaryOp {
                operator,
                left,
                right,
            } => self.validate_binary_op(operator, left, right, path, env),
            Expression::UnaryOp { operator, expr } => {
                self.validate_unary_op(operator, expr, path, env)
            }
            Expression::CollectionLiteral {
                kind,
                items,
                entries,
            } => self.validate_collection_literal(kind, items, entries, path, env),
        }
    }

    fn infer_field(
        &mut self,
        base: &str,
        name: &str,
        path: &str,
        env: &BTreeMap<String, InferredType>,
    ) -> InferredType {
        let Some(base_type) = inferred_type_name(
            env.get(base).unwrap_or(&InferredType::Unknown),
            &self.artifact.export.name,
        ) else {
            return InferredType::Unknown;
        };
        let Some((_, owner)) = self
            .index
            .local_artifact(&base_type, &self.artifact.export.name)
        else {
            return InferredType::Unknown;
        };
        let field_map = artifact_struct_fields(owner);
        let Some(field) = field_map.get(name) else {
            self.issue(
                path,
                format!("field `{name}` is not declared on `{}`", owner.export.name),
            );
            return InferredType::Unknown;
        };
        InferredType::Known(field.rust.clone())
    }

    fn validate_struct_construction(
        &mut self,
        type_name: &str,
        fields: &[StructFieldValue],
        path: &str,
        env: &BTreeMap<String, InferredType>,
    ) -> InferredType {
        let resolved_type = replace_self_type(type_name, &self.artifact.export.name);
        let Some((_, target)) = self
            .index
            .local_artifact(&resolved_type, &self.artifact.export.name)
        else {
            for (idx, field) in fields.iter().enumerate() {
                self.infer_expression(&field.expr, &format!("{path}.fields[{idx}].expr"), env);
            }
            return InferredType::Known(resolved_type);
        };

        let field_map = artifact_struct_fields(target);

        let mut seen = BTreeSet::new();
        for (idx, field) in fields.iter().enumerate() {
            let expr_path = format!("{path}.fields[{idx}]");
            let actual = self.infer_expression(&field.expr, &format!("{expr_path}.expr"), env);
            let Some(expected) = field_map.get(&field.name) else {
                self.issue(
                    &expr_path,
                    format!(
                        "field `{}` is not declared on `{}`",
                        field.name, target.export.name
                    ),
                );
                continue;
            };
            if !seen.insert(field.name.clone()) {
                self.issue(
                    &expr_path,
                    format!(
                        "field `{}` is assigned more than once in `{}` construction",
                        field.name, target.export.name
                    ),
                );
            }
            if !types_compatible_known(&expected.rust, &actual, &self.artifact.export.name, true) {
                self.issue(
                    &expr_path,
                    format!(
                        "field `{}` on `{}` expects `{}` but got {}",
                        field.name,
                        target.export.name,
                        replace_self_type(&expected.rust, &self.artifact.export.name),
                        render_inferred_type(&actual)
                    ),
                );
            }
        }

        let required = field_map.keys().cloned().collect::<BTreeSet<_>>();
        let missing = required.difference(&seen).cloned().collect::<Vec<_>>();
        if !missing.is_empty() {
            self.issue(
                path,
                format!(
                    "construction of `{}` is missing field(s): {}",
                    target.export.name,
                    missing.join(", ")
                ),
            );
        }

        InferredType::Known(target.export.name.clone())
    }

    fn validate_role_method_call(
        &mut self,
        role: &str,
        method: &str,
        args: &[Expression],
        path: &str,
        env: &BTreeMap<String, InferredType>,
    ) -> InferredType {
        let Some(role_spec) = self.artifact.roles.iter().find(|item| item.name == role) else {
            self.issue(
                path,
                format!("role `{role}` is not declared on this artifact"),
            );
            return InferredType::Unknown;
        };
        let Some(method_spec) = role_spec.methods.iter().find(|item| item.name == method) else {
            self.issue(
                path,
                format!("role method `{role}.{method}` is not declared on this artifact"),
            );
            return InferredType::Unknown;
        };
        self.validate_argument_list(
            &format!("role method `{role}.{method}`"),
            &method_spec
                .parameters
                .iter()
                .skip(1)
                .cloned()
                .collect::<Vec<_>>(),
            args,
            &format!("{path}.args"),
            env,
        );
        method_spec
            .return_status
            .rust()
            .map(|rust| InferredType::Known(rust.to_string()))
            .unwrap_or(InferredType::Unknown)
    }

    fn validate_local_method_call(
        &mut self,
        name: &str,
        args: &[Expression],
        path: &str,
        env: &BTreeMap<String, InferredType>,
    ) -> InferredType {
        let Some((owner_raw, method_name)) = name.rsplit_once("::") else {
            for (idx, arg) in args.iter().enumerate() {
                self.infer_expression(arg, &format!("{path}.args[{idx}]"), env);
            }
            return InferredType::Unknown;
        };
        let owner_type = replace_self_type(owner_raw, &self.artifact.export.name);
        let Some((_, owner)) = self
            .index
            .local_artifact(&owner_type, &self.artifact.export.name)
        else {
            for (idx, arg) in args.iter().enumerate() {
                self.infer_expression(arg, &format!("{path}.args[{idx}]"), env);
            }
            return InferredType::Unknown;
        };

        if method_name == "new"
            && owner
                .constructor
                .as_ref()
                .is_some_and(|ctor| ctor.kind == "all_fields")
        {
            let parameters = owner
                .fields
                .iter()
                .filter_map(|field| {
                    field.type_status.rust().map(|rust| ParameterSpec {
                        name: field.name.clone(),
                        type_status: crate::prepared::ValueStatus::resolved(
                            rust.to_string(),
                            "prepare.contracts",
                        ),
                    })
                })
                .collect::<Vec<_>>();
            self.validate_argument_list(
                &format!("constructor `{}::new`", owner.export.name),
                &parameters,
                args,
                &format!("{path}.args"),
                env,
            );
            return InferredType::Known(owner.export.name.clone());
        }

        if let Some(contract) = public_method_contract(owner, method_name) {
            match contract.receiver {
                ReceiverKind::None => {
                    self.validate_argument_types(
                        &format!("associated method `{}`", contract.rendered_signature),
                        &contract.parameters,
                        args,
                        &format!("{path}.args"),
                        env,
                    );
                    InferredType::Known(contract.return_type)
                }
                _ => {
                    self.issue(
                        path,
                        format!(
                            "call uses associated syntax for instance method `{}`",
                            contract.rendered_signature
                        ),
                    );
                    for (idx, arg) in args.iter().enumerate() {
                        self.infer_expression(arg, &format!("{path}.args[{idx}]"), env);
                    }
                    InferredType::Known(contract.return_type)
                }
            }
        } else {
            for (idx, arg) in args.iter().enumerate() {
                self.infer_expression(arg, &format!("{path}.args[{idx}]"), env);
            }
            self.issue(
                path,
                format!(
                    "method `{}` is not declared on local type `{}`",
                    method_name, owner.export.name
                ),
            );
            InferredType::Unknown
        }
    }

    fn validate_instance_method_call(
        &mut self,
        receiver: &Expression,
        method: &str,
        args: &[Expression],
        path: &str,
        env: &BTreeMap<String, InferredType>,
    ) -> InferredType {
        let receiver_type = self.infer_expression(receiver, &format!("{path}.receiver"), env);
        let Some(receiver_rust) = inferred_type_name(&receiver_type, &self.artifact.export.name)
        else {
            for (idx, arg) in args.iter().enumerate() {
                self.infer_expression(arg, &format!("{path}.args[{idx}]"), env);
            }
            return InferredType::Unknown;
        };
        let Some((_, owner)) = self
            .index
            .local_artifact(&receiver_rust, &self.artifact.export.name)
        else {
            for (idx, arg) in args.iter().enumerate() {
                self.infer_expression(arg, &format!("{path}.args[{idx}]"), env);
            }
            return InferredType::Unknown;
        };

        if let Some(contract) = public_method_contract(owner, method) {
            match contract.receiver {
                ReceiverKind::None => {
                    self.issue(
                        path,
                        format!(
                            "call uses instance syntax for associated method `{}`",
                            contract.rendered_signature
                        ),
                    );
                }
                _ => {}
            }
            self.validate_argument_types(
                &format!("method `{}`", contract.rendered_signature),
                &contract.parameters,
                args,
                &format!("{path}.args"),
                env,
            );
            InferredType::Known(contract.return_type)
        } else {
            for (idx, arg) in args.iter().enumerate() {
                self.infer_expression(arg, &format!("{path}.args[{idx}]"), env);
            }
            self.issue(
                path,
                format!(
                    "method `{}` is not declared on local type `{}`",
                    method, owner.export.name
                ),
            );
            InferredType::Unknown
        }
    }

    fn validate_binary_op(
        &mut self,
        operator: &str,
        left: &Expression,
        right: &Expression,
        path: &str,
        env: &BTreeMap<String, InferredType>,
    ) -> InferredType {
        let left_type = self.infer_expression(left, &format!("{path}.left"), env);
        let right_type = self.infer_expression(right, &format!("{path}.right"), env);

        match operator {
            "==" | "!=" | ">" | "<" | ">=" | "<=" => InferredType::Known("bool".to_string()),
            "+" | "-" | "*" | "/" => {
                if !numeric_compatible(&left_type) || !numeric_compatible(&right_type) {
                    self.issue(
                        path,
                        format!(
                            "operator `{operator}` expects numeric operands but got {} and {}",
                            render_inferred_type(&left_type),
                            render_inferred_type(&right_type)
                        ),
                    );
                    return InferredType::Unknown;
                }
                if matches!(left_type, InferredType::IntegerLiteral) {
                    return right_type;
                }
                if matches!(right_type, InferredType::IntegerLiteral) {
                    return left_type;
                }
                left_type
            }
            _ => InferredType::Unknown,
        }
    }

    fn validate_unary_op(
        &mut self,
        operator: &str,
        expr: &Expression,
        path: &str,
        env: &BTreeMap<String, InferredType>,
    ) -> InferredType {
        let inner = self.infer_expression(expr, &format!("{path}.expr"), env);
        if operator == "!" && !bool_compatible(&inner, &self.artifact.export.name) {
            self.issue(
                path,
                format!(
                    "operator `!` expects `bool` but got {}",
                    render_inferred_type(&inner)
                ),
            );
        }
        match operator {
            "!" => InferredType::Known("bool".to_string()),
            _ => InferredType::Unknown,
        }
    }

    fn validate_collection_literal(
        &mut self,
        kind: &str,
        items: &[Expression],
        entries: &[CollectionEntry],
        path: &str,
        env: &BTreeMap<String, InferredType>,
    ) -> InferredType {
        match kind {
            "vec" => {
                let item_type = infer_homogeneous_items(self, items, &format!("{path}.items"), env);
                item_type
                    .map(|inner| InferredType::Known(format!("Vec<{inner}>")))
                    .unwrap_or(InferredType::Unknown)
            }
            "hash_map" => {
                let key_type =
                    infer_homogeneous_entries(self, entries, &format!("{path}.entries"), env, true);
                let value_type = infer_homogeneous_entries(
                    self,
                    entries,
                    &format!("{path}.entries"),
                    env,
                    false,
                );
                match (key_type, value_type) {
                    (Some(key), Some(value)) => {
                        InferredType::Known(format!("HashMap<{key}, {value}>"))
                    }
                    _ => InferredType::Unknown,
                }
            }
            _ => {
                for (idx, item) in items.iter().enumerate() {
                    self.infer_expression(item, &format!("{path}.items[{idx}]"), env);
                }
                for (idx, entry) in entries.iter().enumerate() {
                    self.infer_expression(&entry.key, &format!("{path}.entries[{idx}].key"), env);
                    self.infer_expression(
                        &entry.value,
                        &format!("{path}.entries[{idx}].value"),
                        env,
                    );
                }
                InferredType::Unknown
            }
        }
    }

    fn validate_argument_list(
        &mut self,
        callee: &str,
        parameters: &[ParameterSpec],
        args: &[Expression],
        path: &str,
        env: &BTreeMap<String, InferredType>,
    ) {
        let named = parameters
            .iter()
            .filter_map(|parameter| {
                parameter.type_status.rust().map(|rust| NamedType {
                    name: parameter.name.clone(),
                    rust: rust.to_string(),
                })
            })
            .collect::<Vec<_>>();
        self.validate_argument_types(callee, &named, args, path, env);
    }

    fn validate_argument_types(
        &mut self,
        callee: &str,
        parameters: &[NamedType],
        args: &[Expression],
        path: &str,
        env: &BTreeMap<String, InferredType>,
    ) {
        if parameters.len() != args.len() {
            self.issue(
                path,
                format!(
                    "{callee} expects {} argument(s) but got {}",
                    parameters.len(),
                    args.len()
                ),
            );
        }
        for (idx, arg) in args.iter().enumerate() {
            let actual = self.infer_expression(arg, &format!("{path}[{idx}]"), env);
            let Some(expected) = parameters.get(idx) else {
                continue;
            };
            if !types_compatible_known(&expected.rust, &actual, &self.artifact.export.name, true) {
                self.issue(
                    &format!("{path}[{idx}]"),
                    format!(
                        "{callee} parameter `{}` expects `{}` but got {}",
                        expected.name,
                        replace_self_type(&expected.rust, &self.artifact.export.name),
                        render_inferred_type(&actual)
                    ),
                );
            }
        }
    }

    fn issue(&mut self, path: &str, message: String) {
        self.issues.push(ContractIssue {
            artifact_index: self.artifact_index,
            ambiguity: Ambiguity {
                path: path.to_string(),
                severity: "blocking".to_string(),
                message,
                source_line: None,
            },
        });
    }
}

fn validate_collab_sync_wrappers(
    artifact_index: usize,
    artifact: &PreparedArtifact,
    index: &ArtifactIndex<'_>,
    issues: &mut Vec<ContractIssue>,
) {
    for (role_idx, role) in artifact.roles.iter().enumerate() {
        let Some(role_rust) = role.type_status.rust() else {
            continue;
        };
        let Some((_, collaborator)) = index.local_artifact(role_rust, &artifact.export.name) else {
            continue;
        };
        for (method_idx, method) in role.methods.iter().enumerate() {
            if method.signature.source.as_deref() != Some("prepare.collab_sync") {
                continue;
            }
            let base_path = format!("roles[{role_idx}].methods[{method_idx}]");
            let Some(target) = public_method_contract(collaborator, &method.name) else {
                issues.push(ContractIssue {
                    artifact_index,
                    ambiguity: Ambiguity {
                        path: format!("{base_path}.signature"),
                        severity: "blocking".to_string(),
                        message: format!(
                            "role method `{}` syncs from collaborator `{}` but that collaborator has no public `{}` method/getter",
                            method.name, collaborator.export.name, method.name
                        ),
                        source_line: None,
                    },
                });
                continue;
            };
            if target.receiver == ReceiverKind::None {
                issues.push(ContractIssue {
                    artifact_index,
                    ambiguity: Ambiguity {
                        path: format!("{base_path}.signature"),
                        severity: "blocking".to_string(),
                        message: format!(
                            "role method `{}` cannot sync to associated method `{}`",
                            method.name, target.rendered_signature
                        ),
                        source_line: None,
                    },
                });
                continue;
            }

            let expected =
                expected_wrapper_contract(role, method, &target, &collaborator.export.name);
            let actual = actual_wrapper_contract(method);
            if !wrapper_contract_matches(&expected, &actual, &artifact.export.name) {
                issues.push(ContractIssue {
                    artifact_index,
                    ambiguity: Ambiguity {
                        path: format!("{base_path}.signature"),
                        severity: "blocking".to_string(),
                        message: format!(
                            "role method wrapper does not match collaborator contract: expected `{}`, found `{}`",
                            expected.rendered_signature, actual.rendered_signature
                        ),
                        source_line: None,
                    },
                });
            }
        }
    }
}

fn validate_body_contracts(
    artifact_index: usize,
    artifact: &PreparedArtifact,
    index: &ArtifactIndex<'_>,
    issues: &mut Vec<ContractIssue>,
) {
    let mut validator = MethodValidator {
        artifact_index,
        artifact,
        index,
        issues,
    };
    for (idx, method) in artifact.functionalities.iter().enumerate() {
        validator.validate_method(method, &format!("functionalities[{idx}]"));
    }
    for (role_idx, role) in artifact.roles.iter().enumerate() {
        for (method_idx, method) in role.methods.iter().enumerate() {
            validator.validate_method(method, &format!("roles[{role_idx}].methods[{method_idx}]"));
        }
    }
}

fn expected_wrapper_contract(
    role: &RoleSpec,
    method: &MethodSpec,
    target: &PublicMethodContract,
    collaborator_export: &str,
) -> PublicMethodContract {
    let role_param_type = match target.receiver {
        ReceiverKind::Ref => format!("&{collaborator_export}"),
        ReceiverKind::MutRef => format!("&mut {collaborator_export}"),
        ReceiverKind::Owned => collaborator_export.to_string(),
        ReceiverKind::None => collaborator_export.to_string(),
    };
    let mut parameters = vec![NamedType {
        name: format!("{}_", role.name),
        rust: role_param_type,
    }];
    parameters.extend(target.parameters.clone());
    PublicMethodContract {
        receiver: ReceiverKind::Ref,
        parameters: parameters.clone(),
        return_type: target.return_type.clone(),
        rendered_signature: render_signature(
            &method.name,
            Some("&self"),
            &parameters,
            &target.return_type,
        ),
    }
}

fn actual_wrapper_contract(method: &MethodSpec) -> PublicMethodContract {
    let parameters = method
        .parameters
        .iter()
        .filter_map(|parameter| {
            parameter.type_status.rust().map(|rust| NamedType {
                name: parameter.name.clone(),
                rust: rust.to_string(),
            })
        })
        .collect::<Vec<_>>();
    PublicMethodContract {
        receiver: receiver_kind(method.receiver.as_deref()),
        parameters: parameters.clone(),
        return_type: method
            .return_status
            .rust()
            .map(str::to_string)
            .unwrap_or_else(|| "Unknown".to_string()),
        rendered_signature: render_signature(
            &method.name,
            method.receiver.as_deref(),
            &parameters,
            method.return_status.rust().unwrap_or("Unknown"),
        ),
    }
}

fn wrapper_contract_matches(
    expected: &PublicMethodContract,
    actual: &PublicMethodContract,
    current_export: &str,
) -> bool {
    if expected.receiver != actual.receiver {
        return false;
    }
    if expected.parameters.len() != actual.parameters.len() {
        return false;
    }
    if !types_compatible_known(
        &expected.return_type,
        &InferredType::Known(actual.return_type.clone()),
        current_export,
        false,
    ) {
        return false;
    }
    expected
        .parameters
        .iter()
        .zip(&actual.parameters)
        .all(|(left, right)| {
            types_compatible_known(
                &left.rust,
                &InferredType::Known(right.rust.clone()),
                current_export,
                false,
            )
        })
}

fn public_method_contract(artifact: &PreparedArtifact, name: &str) -> Option<PublicMethodContract> {
    if let Some(method) = artifact
        .functionalities
        .iter()
        .find(|method| method.name == name)
    {
        let parameters = method
            .parameters
            .iter()
            .filter_map(|parameter| {
                parameter.type_status.rust().map(|rust| NamedType {
                    name: parameter.name.clone(),
                    rust: replace_self_type(rust, &artifact.export.name),
                })
            })
            .collect::<Vec<_>>();
        let return_type = replace_self_type(method.return_status.rust()?, &artifact.export.name);
        return Some(PublicMethodContract {
            receiver: receiver_kind(method.receiver.as_deref()),
            parameters: parameters.clone(),
            return_type: return_type.clone(),
            rendered_signature: render_signature(
                &method.name,
                method.receiver.as_deref(),
                &parameters,
                &return_type,
            ),
        });
    }
    let getter = artifact.getters.iter().find(|getter| getter.name == name)?;
    let field = artifact
        .fields
        .iter()
        .find(|field| field.name == getter.field)?;
    let field_type = field.type_status.rust()?.to_string();
    let return_type = getter_return_type(getter, &field_type);
    Some(PublicMethodContract {
        receiver: ReceiverKind::Ref,
        parameters: Vec::new(),
        return_type: return_type.clone(),
        rendered_signature: render_signature(&getter.name, Some("&self"), &[], &return_type),
    })
}

fn getter_return_type(getter: &GetterSpec, field_type: &str) -> String {
    match getter.mode.as_str() {
        "copy" => field_type.to_string(),
        _ => format!("&{field_type}"),
    }
}

fn artifact_struct_fields(artifact: &PreparedArtifact) -> BTreeMap<String, FieldContract> {
    let mut fields = artifact
        .fields
        .iter()
        .filter_map(|field| {
            field.type_status.rust().map(|rust| {
                (
                    field.name.clone(),
                    FieldContract {
                        rust: rust.to_string(),
                    },
                )
            })
        })
        .collect::<BTreeMap<_, _>>();
    if !fields.is_empty() {
        return fields;
    }
    for role in &artifact.roles {
        if let Some(rust) = role.type_status.rust() {
            fields.insert(
                role.name.clone(),
                FieldContract {
                    rust: rust.to_string(),
                },
            );
        }
    }
    for prop in &artifact.props {
        if let Some(rust) = prop.type_status.rust() {
            fields.insert(
                prop.name.clone(),
                FieldContract {
                    rust: rust.to_string(),
                },
            );
        }
    }
    for collaborator in &artifact.collaborators {
        if let Some(rust) = collaborator.type_status.rust() {
            fields.insert(
                collaborator.name.clone(),
                FieldContract {
                    rust: rust.to_string(),
                },
            );
        }
    }
    fields
}

fn receiver_kind(receiver: Option<&str>) -> ReceiverKind {
    match receiver.map(str::trim) {
        Some("&self") => ReceiverKind::Ref,
        Some("&mut self") => ReceiverKind::MutRef,
        Some("self") | Some("mut self") => ReceiverKind::Owned,
        None => ReceiverKind::None,
        _ => ReceiverKind::Ref,
    }
}

fn render_signature(
    name: &str,
    receiver: Option<&str>,
    parameters: &[NamedType],
    return_type: &str,
) -> String {
    let mut parts = Vec::new();
    if let Some(receiver) = receiver {
        parts.push(receiver.to_string());
    }
    parts.extend(
        parameters
            .iter()
            .map(|parameter| format!("{}: {}", parameter.name, parameter.rust)),
    );
    format!("{name}({}) -> {return_type}", parts.join(", "))
}

fn types_compatible(
    expected: &InferredType,
    actual: &InferredType,
    current_export: &str,
    relax_references: bool,
) -> bool {
    match expected {
        InferredType::Unknown => true,
        InferredType::IntegerLiteral => matches!(actual, InferredType::IntegerLiteral),
        InferredType::Known(expected) => {
            types_compatible_known(expected, actual, current_export, relax_references)
        }
    }
}

fn types_compatible_known(
    expected: &str,
    actual: &InferredType,
    current_export: &str,
    relax_references: bool,
) -> bool {
    match actual {
        InferredType::Unknown => true,
        InferredType::IntegerLiteral => is_integer_type(expected),
        InferredType::Known(actual) => {
            let expected = normalized_type(&replace_self_type(expected, current_export));
            let actual = normalized_type(&replace_self_type(actual, current_export));
            if expected == actual {
                return true;
            }
            if relax_references {
                return strip_leading_reference(&expected) == strip_leading_reference(&actual);
            }
            false
        }
    }
}

fn inferred_type_name(value: &InferredType, current_export: &str) -> Option<String> {
    match value {
        InferredType::Known(rust) => Some(replace_self_type(rust, current_export)),
        _ => None,
    }
}

fn foreach_item_type(value: &InferredType, current_export: &str) -> Option<String> {
    let ty = inferred_type_name(value, current_export)?;
    let normalized = normalized_type(&ty);
    let simple = simple_type_name(&normalized);
    let inner = first_generic_argument(&normalized)?;
    match simple.as_str() {
        "Vec" | "VecDeque" | "HashSet" | "BTreeSet" => Some(inner),
        _ => None,
    }
}

fn infer_homogeneous_items(
    validator: &mut MethodValidator<'_, '_>,
    items: &[Expression],
    path: &str,
    env: &BTreeMap<String, InferredType>,
) -> Option<String> {
    let mut resolved: Option<String> = None;
    for (idx, item) in items.iter().enumerate() {
        let ty = validator.infer_expression(item, &format!("{path}[{idx}]"), env);
        let Some(name) = inferred_type_name(&ty, &validator.artifact.export.name) else {
            continue;
        };
        if let Some(existing) = &resolved {
            if normalized_type(existing) != normalized_type(&name) {
                return None;
            }
        } else {
            resolved = Some(name);
        }
    }
    resolved
}

fn infer_homogeneous_entries(
    validator: &mut MethodValidator<'_, '_>,
    entries: &[CollectionEntry],
    path: &str,
    env: &BTreeMap<String, InferredType>,
    keys: bool,
) -> Option<String> {
    let mut resolved: Option<String> = None;
    for (idx, entry) in entries.iter().enumerate() {
        let expr = if keys { &entry.key } else { &entry.value };
        let part = if keys { "key" } else { "value" };
        let ty = validator.infer_expression(expr, &format!("{path}[{idx}].{part}"), env);
        let Some(name) = inferred_type_name(&ty, &validator.artifact.export.name) else {
            continue;
        };
        if let Some(existing) = &resolved {
            if normalized_type(existing) != normalized_type(&name) {
                return None;
            }
        } else {
            resolved = Some(name);
        }
    }
    resolved
}

fn numeric_compatible(value: &InferredType) -> bool {
    match value {
        InferredType::Unknown | InferredType::IntegerLiteral => true,
        InferredType::Known(rust) => is_integer_type(rust) || is_float_type(rust),
    }
}

fn bool_compatible(value: &InferredType, current_export: &str) -> bool {
    match value {
        InferredType::Unknown => true,
        InferredType::Known(rust) => {
            normalized_type(&replace_self_type(rust, current_export)) == "bool"
        }
        InferredType::IntegerLiteral => false,
    }
}

fn is_integer_type(rust: &str) -> bool {
    matches!(
        strip_leading_reference(&normalized_type(rust)).as_str(),
        "u8" | "u16"
            | "u32"
            | "u64"
            | "u128"
            | "usize"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "i128"
            | "isize"
    )
}

fn is_float_type(rust: &str) -> bool {
    matches!(
        strip_leading_reference(&normalized_type(rust)).as_str(),
        "f32" | "f64"
    )
}

fn normalized_type(rust: &str) -> String {
    rust.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn strip_leading_reference(rust: &str) -> String {
    let mut value = rust.trim();
    while let Some(rest) = value.strip_prefix('&') {
        value = rest.trim_start();
        if let Some(rest) = value.strip_prefix("mut ") {
            value = rest.trim_start();
        }
        if let Some(rest) = value.strip_prefix('\'') {
            let offset = rest
                .find(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
                .unwrap_or(rest.len());
            value = rest[offset..].trim_start();
        }
    }
    value.to_string()
}

fn replace_self_type(rust: &str, current_export: &str) -> String {
    rust.replace("Self", current_export)
}

fn simple_type_name(raw: &str) -> String {
    let stripped = strip_leading_reference(raw);
    let last_segment = stripped.rsplit("::").next().unwrap_or(&stripped);
    let generic_start = last_segment.find('<').unwrap_or(last_segment.len());
    last_segment[..generic_start].trim().to_string()
}

fn first_generic_argument(rust: &str) -> Option<String> {
    let open = rust.find('<')?;
    let inner = &rust[open + 1..];
    let mut depth = 0i32;
    let mut out = String::new();
    for ch in inner.chars() {
        match ch {
            '<' => {
                depth += 1;
                out.push(ch);
            }
            '>' => {
                if depth == 0 {
                    break;
                }
                depth -= 1;
                out.push(ch);
            }
            ',' if depth == 0 => break,
            _ => out.push(ch),
        }
    }
    let trimmed = out.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn render_inferred_type(value: &InferredType) -> String {
    match value {
        InferredType::Unknown => "unknown type".to_string(),
        InferredType::IntegerLiteral => "integer literal".to_string(),
        InferredType::Known(rust) => format!("`{rust}`"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prepared::{
        Body, ConstructorPolicy, ExportInfo, FieldSpec, MethodSpec, PREPARED_SCHEMA_VERSION,
        ParameterSpec, RoleSpec, SourceInfo, ValueStatus,
    };

    fn artifact(kind: &str, path: &str, export: &str, mutable: bool) -> PreparedArtifact {
        PreparedArtifact {
            schema: PREPARED_SCHEMA_VERSION.to_string(),
            source: SourceInfo {
                path: path.to_string(),
                kind: kind.to_string(),
                title: export.to_string(),
            },
            export: ExportInfo {
                name: export.to_string(),
            },
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

    fn resolved(rust: &str) -> crate::prepared::ValueStatus {
        ValueStatus::resolved(rust.to_string(), "test")
    }

    fn method(
        name: &str,
        receiver: Option<&str>,
        params: &[(&str, &str)],
        returns: &str,
    ) -> MethodSpec {
        MethodSpec {
            name: name.to_string(),
            signature: resolved(&render_signature(
                name,
                receiver,
                &params
                    .iter()
                    .map(|(name, rust)| NamedType {
                        name: (*name).to_string(),
                        rust: (*rust).to_string(),
                    })
                    .collect::<Vec<_>>(),
                returns,
            )),
            receiver: receiver.map(str::to_string),
            parameters: params
                .iter()
                .map(|(name, rust)| ParameterSpec {
                    name: (*name).to_string(),
                    type_status: resolved(rust),
                })
                .collect(),
            return_status: resolved(returns),
            flow: Vec::new(),
            extensions: Vec::new(),
            guarantee: Vec::new(),
            references: None,
            body: None,
        }
    }

    #[test]
    fn prepare_collab_sync_wrapper_drift_is_rejected() {
        let mut command = artifact(
            "context",
            "contexts/command.md",
            "CommandInputContext",
            true,
        );
        command
            .functionalities
            .push(method("capture", Some("&mut self"), &[], "()"));

        let mut game_loop = artifact("context", "contexts/game_loop.md", "GameLoopContext", true);
        let mut wrapper = method(
            "capture",
            Some("&self"),
            &[("command_", "&CommandInputContext")],
            "()",
        );
        wrapper.signature.source = Some("prepare.collab_sync".to_string());
        game_loop.roles.push(RoleSpec {
            name: "command".to_string(),
            purpose: "input".to_string(),
            expected_behavior: "captures".to_string(),
            type_status: resolved("CommandInputContext"),
            methods: vec![wrapper],
        });

        let issues = collect_contract_issues(&[command, game_loop]);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].artifact_index, 1);
        assert_eq!(issues[0].ambiguity.path, "roles[0].methods[0].signature");
        assert!(
            issues[0]
                .ambiguity
                .message
                .contains("role method wrapper does not match collaborator contract")
        );
    }

    #[test]
    fn body_ir_constructor_argument_mismatch_is_rejected() {
        let mut board = artifact("data", "data/Board.md", "Board", false);
        board.constructor = Some(ConstructorPolicy {
            kind: "all_fields".to_string(),
        });
        board.fields = vec![
            FieldSpec {
                name: "width".to_string(),
                meaning: "width".to_string(),
                type_status: resolved("u32"),
                getter_accessible: true,
                notes: Vec::new(),
            },
            FieldSpec {
                name: "height".to_string(),
                meaning: "height".to_string(),
                type_status: resolved("u32"),
                getter_accessible: true,
                notes: Vec::new(),
            },
        ];

        let mut app = artifact("app", "app.md", "ExampleApp", true);
        let mut main = method("main", None, &[], "()");
        main.body = Some(Body {
            steps: vec![Statement::Let {
                name: "board".to_string(),
                expr: Expression::CallLocalMethod {
                    name: "Board::new".to_string(),
                    args: vec![
                        Expression::Literal {
                            kind: "string".to_string(),
                            value: "wide".to_string(),
                        },
                        Expression::Literal {
                            kind: "integer".to_string(),
                            value: "20".to_string(),
                        },
                    ],
                },
            }],
        });
        app.functionalities.push(main);

        let issues = collect_contract_issues(&[board, app]);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].artifact_index, 1);
        assert_eq!(
            issues[0].ambiguity.path,
            "functionalities[0].body.steps[0].expr.args[0]"
        );
        assert!(
            issues[0]
                .ambiguity
                .message
                .contains("constructor `Board::new`")
        );
        assert!(issues[0].ambiguity.message.contains("expects `u32`"));
    }

    #[test]
    fn prose_only_method_is_ignored() {
        let mut artifact = artifact("context", "contexts/example.md", "ExampleContext", true);
        let mut method = method("run", Some("&self"), &[], "()");
        method.body = None;
        method.flow = vec!["Keep running until complete.".to_string()];
        artifact.functionalities.push(method);

        let issues = collect_contract_issues(&[artifact]);
        assert!(issues.is_empty());
    }
}
