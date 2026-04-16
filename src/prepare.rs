use crate::build_tracker::{BuildTracker, hash_file};
use crate::draft_parser::{
    AppDraft, CompositeDraft, DataDraft, DraftDocument, FunctionalityDraft, RoleMethodGroup,
    parse_draft_file,
};
use crate::fix_agent;
use crate::prepared::{
    Ambiguity, Body, CollaboratorSpec, ConstructorPolicy, Evidence, ExportInfo, Expression,
    FieldSpec, GetterSpec, MethodReferences, MethodSpec, ParameterSpec, PreparedArtifact, PropSpec,
    RoleSpec, SourceInfo, Statement, StructFieldValue, ValueStatus, VariantSpec,
};
use crate::workspace::Workspace;
use anyhow::{Context, Result};
use regex::Regex;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct PrepareOptions {
    pub selection: crate::workspace::Selection,
    pub profile: Option<String>,
    pub fix: bool,
    pub verbose: bool,
    pub debug: bool,
    pub dry_run: bool,
}

#[derive(Debug, Clone)]
struct DraftCatalog {
    exports: BTreeSet<String>,
    normalized: BTreeMap<String, Vec<String>>,
    data_drafts: BTreeMap<String, DataDraft>,
}

#[derive(Debug, Clone, Default)]
struct BodyParseContext {
    /// Maps role field name → set of method names declared on that role.
    role_methods: BTreeMap<String, BTreeSet<String>>,
    /// Role field names declared on the context (keys of `role_methods`, kept separately for
    /// clarity and so role-only contexts with no role methods still work).
    role_names: BTreeSet<String>,
    /// Prop names declared on the context.
    prop_names: BTreeSet<String>,
}

#[derive(Debug, Default, Deserialize)]
struct TypesManifest {
    #[serde(default)]
    primitives: Vec<String>,
    #[serde(default)]
    external_path_prefixes: Vec<String>,
    #[serde(default)]
    allowlists: ManifestAllowlists,
}

#[derive(Debug, Default, Deserialize)]
struct ManifestAllowlists {
    #[serde(default)]
    data: Vec<String>,
    #[serde(default)]
    projection: Vec<String>,
    #[serde(default)]
    context: Vec<String>,
}

pub fn prepare_workspace(workspace: &Workspace, options: &PrepareOptions) -> Result<()> {
    let selected_paths = workspace.raw_draft_paths(&options.selection)?;
    let all_docs = load_supported_documents(workspace)?;
    let catalog = build_catalog(&all_docs);
    let types_manifest = load_types_manifest(workspace)?;
    let mut tracker = BuildTracker::load(&workspace.root)?;
    let mut wrote = 0usize;
    let mut blocking_total = 0usize;

    if options.verbose {
        println!("Preparing {} draft(s)", selected_paths.len());
    }
    if options.verbose {
        if let Some(profile) = &options.profile {
            println!("Profile `{profile}` is reserved; v2 prepare runs deterministically.");
        }
    }

    for path in &selected_paths {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let draft = parse_draft_file(path, &workspace.drafts_dir, &content)?;
        let output_path = workspace.prepared_output_path(path)?;
        let input_hash = hash_file(path)?;
        let track_key = format!("prepare:{}", draft.info().relative_path());
        if tracker.is_current("prepare", &track_key, &input_hash) && output_path.exists() {
            if options.verbose {
                println!("skip {}", draft.info().relative_path());
            }
            continue;
        }
        let mut prepared = prepare_document(&draft, &catalog)?;
        prepared.propagate_resolved_types();
        prepared.refresh_ambiguity_index();
        enrich_ambiguity_lines(&mut prepared, &content);
        let has_blockers = prepared.blocking_ambiguities().next().is_some();

        if has_blockers && options.fix {
            let available_types =
                available_types_for_draft(types_manifest.as_ref(), draft.info().kind, &catalog);
            let fixed_count = fix_agent::fix_ambiguities(
                &content,
                &available_types,
                &mut prepared,
                options.verbose,
            )?;
            if fixed_count > 0 {
                prepared.propagate_resolved_types();
                prepared.refresh_ambiguity_index();
                if options.verbose {
                    eprintln!(
                        "fix-agent: applied {} fix(es) for {}",
                        fixed_count,
                        draft.info().relative_path()
                    );
                }
            }
        }

        let blockers: Vec<_> = prepared.blocking_ambiguities().collect();
        if !blockers.is_empty() {
            blocking_total += blockers.len();
            let draft_path = path.strip_prefix(&workspace.root).unwrap_or(path);
            for ambiguity in blockers {
                let location = match ambiguity.source_line {
                    Some(line) => format!("{}:{}", draft_path.display(), line),
                    None => format!("{}", draft_path.display()),
                };
                eprintln!("error[prepare]: {}", ambiguity.message);
                eprintln!("  --> {} ({})", location, ambiguity.path);
            }
        }
        let yaml = serde_yaml::to_string(&prepared)?;
        if options.dry_run {
            if options.verbose {
                println!("[dry-run] would write {}", output_path.display());
            }
        } else {
            if let Some(parent) = output_path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("Failed to create {}", parent.display()))?;
            }
            fs::write(&output_path, yaml)
                .with_context(|| format!("Failed to write {}", output_path.display()))?;
            tracker.update("prepare", track_key, input_hash);
            wrote += 1;
        }

        if options.debug && !options.dry_run {
            write_prepare_debug_dump(workspace, &prepared)?;
        }
    }

    if !options.dry_run {
        tracker.save(&workspace.root)?;
    }
    if options.verbose {
        println!("Prepared {} artifact(s)", wrote);
    }
    if blocking_total > 0 {
        anyhow::bail!(
            "prepare found {} blocking ambiguit{} across draft(s); see errors above",
            blocking_total,
            if blocking_total == 1 { "y" } else { "ies" }
        );
    }
    Ok(())
}

fn load_types_manifest(workspace: &Workspace) -> Result<Option<TypesManifest>> {
    let path = workspace.drafts_dir.join("types-manifest.yml");
    if !path.is_file() {
        return Ok(None);
    }
    let raw =
        fs::read_to_string(&path).with_context(|| format!("Failed to read {}", path.display()))?;
    let manifest = serde_yaml::from_str(&raw)
        .with_context(|| format!("Failed to parse {}", path.display()))?;
    Ok(Some(manifest))
}

fn available_types_for_draft(
    manifest: Option<&TypesManifest>,
    kind: crate::draft_parser::ArtifactKind,
    catalog: &DraftCatalog,
) -> Vec<String> {
    let mut types = BTreeSet::new();
    types.extend(catalog.exports.iter().cloned());

    let Some(manifest) = manifest else {
        return types.into_iter().collect();
    };

    types.extend(manifest.primitives.iter().cloned());
    types.extend(manifest.external_path_prefixes.iter().cloned());

    match kind {
        crate::draft_parser::ArtifactKind::Data => {
            types.extend(manifest.allowlists.data.iter().cloned());
        }
        crate::draft_parser::ArtifactKind::Projection => {
            types.extend(manifest.allowlists.projection.iter().cloned());
        }
        crate::draft_parser::ArtifactKind::Context => {
            types.extend(manifest.allowlists.context.iter().cloned());
        }
        crate::draft_parser::ArtifactKind::App => {
            types.extend(manifest.allowlists.data.iter().cloned());
            types.extend(manifest.allowlists.projection.iter().cloned());
            types.extend(manifest.allowlists.context.iter().cloned());
        }
        crate::draft_parser::ArtifactKind::UnsupportedApi => {}
    }

    types.into_iter().collect()
}

fn enrich_ambiguity_lines(prepared: &mut PreparedArtifact, source: &str) {
    let lines: Vec<&str> = source.lines().collect();
    for ambiguity in &mut prepared.ambiguities {
        if ambiguity.source_line.is_some() {
            continue;
        }
        ambiguity.source_line = guess_ambiguity_line(&ambiguity.path, &ambiguity.message, &lines);
    }
}

fn guess_ambiguity_line(path: &str, message: &str, lines: &[&str]) -> Option<usize> {
    let needle = extract_ambiguity_subject(path, message)?;
    for (idx, line) in lines.iter().enumerate() {
        let lower = line.to_ascii_lowercase();
        if lower.contains(&needle.to_ascii_lowercase()) {
            return Some(idx + 1);
        }
    }
    None
}

fn extract_ambiguity_subject(path: &str, message: &str) -> Option<String> {
    let backtick_re = Regex::new(r"`([^`]+)`").expect("backtick regex");
    if let Some(cap) = backtick_re.captures(message) {
        return Some(cap.get(1).unwrap().as_str().to_string());
    }
    let last_segment = path.rsplit('.').next().unwrap_or(path);
    if last_segment != "type" && last_segment != "body" && last_segment != "signature" {
        return Some(last_segment.to_string());
    }
    None
}

pub fn clear_prepared_outputs(workspace: &Workspace, dry_run: bool) -> Result<()> {
    if dry_run {
        println!(
            "[dry-run] would remove {}",
            workspace.prepared_dir.display()
        );
    } else if workspace.prepared_dir.exists() {
        fs::remove_dir_all(&workspace.prepared_dir)
            .with_context(|| format!("Failed to remove {}", workspace.prepared_dir.display()))?;
    }
    let mut tracker = BuildTracker::load(&workspace.root)?;
    tracker.clear_stage("prepare");
    if !dry_run {
        tracker.save(&workspace.root)?;
    }
    Ok(())
}

fn write_prepare_debug_dump(workspace: &Workspace, prepared: &PreparedArtifact) -> Result<()> {
    let path = workspace
        .state_dir
        .join("debug")
        .join("prepare")
        .join(&prepared.source.path)
        .with_extension("yml");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    let yaml = serde_yaml::to_string(prepared)?;
    fs::write(&path, yaml).with_context(|| format!("Failed to write {}", path.display()))
}

fn load_supported_documents(workspace: &Workspace) -> Result<Vec<DraftDocument>> {
    let mut paths = Vec::new();
    for kind_dir in ["data", "projections", "contexts"] {
        let dir = workspace.drafts_dir.join(kind_dir);
        if dir.is_dir() {
            collect_markdown_files(&dir, &mut paths)?;
        }
    }
    let app = workspace.drafts_dir.join("app.md");
    if app.is_file() {
        paths.push(app);
    }
    paths.sort();

    let mut docs = Vec::new();
    for path in paths {
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        docs.push(parse_draft_file(&path, &workspace.drafts_dir, &content)?);
    }
    Ok(docs)
}

fn collect_markdown_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("read_dir {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_markdown_files(&path, out)?;
            continue;
        }
        if path.extension().and_then(|value| value.to_str()) == Some("md") {
            out.push(path);
        }
    }
    Ok(())
}

fn build_catalog(docs: &[DraftDocument]) -> DraftCatalog {
    let mut exports = BTreeSet::new();
    let mut normalized = BTreeMap::<String, Vec<String>>::new();
    let mut data_drafts = BTreeMap::new();
    for doc in docs {
        let export = export_name(doc.info().title());
        exports.insert(export.clone());
        normalized
            .entry(normalize_symbol(&export))
            .or_default()
            .push(export.clone());
        normalized
            .entry(normalize_symbol(doc.info().title()))
            .or_default()
            .push(export.clone());
        normalized
            .entry(normalize_symbol(doc.info().stem()))
            .or_default()
            .push(export.clone());
        if let DraftDocument::Data(data) = doc {
            data_drafts.insert(export.clone(), data.clone());
        }
    }
    for values in normalized.values_mut() {
        values.sort();
        values.dedup();
    }
    DraftCatalog {
        exports,
        normalized,
        data_drafts,
    }
}

fn prepare_document(draft: &DraftDocument, catalog: &DraftCatalog) -> Result<PreparedArtifact> {
    match draft {
        DraftDocument::Data(data) => prepare_data(data, catalog),
        DraftDocument::Projection(projection) => prepare_composite(projection, catalog),
        DraftDocument::Context(context) => prepare_composite(context, catalog),
        DraftDocument::App(app) => prepare_app(app, catalog),
    }
}

fn prepare_data(draft: &DataDraft, catalog: &DraftCatalog) -> Result<PreparedArtifact> {
    let export = export_name(draft.info.title());
    // Data types are immutable value objects.
    let mut prepared = PreparedArtifact::empty(
        draft.info.kind.as_str(),
        draft.info.relative_path.clone(),
        draft.info.title.clone(),
        export,
        false,
    );
    prepared.source = SourceInfo {
        path: draft.info.relative_path.clone(),
        kind: draft.info.kind.as_str().to_string(),
        title: draft.info.title.clone(),
    };
    prepared.export = ExportInfo {
        name: export_name(draft.info.title()),
    };
    prepared.invariants.extend(draft.rules.clone());
    prepared.derives = infer_derives(&draft.notes);

    if !draft.fields.is_empty() {
        prepared.constructor = Some(ConstructorPolicy {
            kind: "all_fields".to_string(),
        });
        for field in &draft.fields {
            let type_status = resolve_data_field_type(
                field.name.as_str(),
                &[&field.meaning, &field.notes],
                catalog,
            );
            prepared.fields.push(FieldSpec {
                name: sanitize_identifier(&field.name),
                meaning: field.meaning.clone(),
                type_status: type_status.clone(),
                getter_accessible: field.getter_accessible,
                notes: non_empty_lines(&field.notes),
            });
            if field.getter_accessible {
                prepared.getters.push(GetterSpec {
                    name: sanitize_identifier(&field.name),
                    field: sanitize_identifier(&field.name),
                    mode: getter_mode(type_status.rust()).to_string(),
                });
            }
        }
    }
    for variant in &draft.variants {
        prepared.variants.push(VariantSpec {
            name: export_name(&variant.name),
            meaning: variant.meaning.clone(),
            notes: non_empty_lines(&variant.notes),
        });
    }
    if !draft.functionality_sections.is_empty() {
        prepared.ambiguities.push(Ambiguity {
            path: "functionalities".to_string(),
            severity: "blocking".to_string(),
            message: "data functionalities require richer prepare support in v1".to_string(),
            source_line: None,
        });
    }
    Ok(prepared)
}

fn prepare_composite(draft: &CompositeDraft, catalog: &DraftCatalog) -> Result<PreparedArtifact> {
    let export = export_name(draft.info.title());
    // Contexts are mutable (they orchestrate a use case and may update their own state).
    // Projections are immutable (pure read-only views).
    let is_context = draft.info.kind == crate::draft_parser::ArtifactKind::Context;
    let mut prepared = PreparedArtifact::empty(
        draft.info.kind.as_str(),
        draft.info.relative_path.clone(),
        draft.info.title.clone(),
        export.clone(),
        is_context,
    );
    prepared.invariants.extend(draft.notes.clone());
    let mut role_method_lookup = BTreeMap::<String, BTreeSet<String>>::new();

    for role in &draft.roles {
        let role_type = if let Some(explicit) = &role.explicit_type {
            ValueStatus::resolved(explicit.clone(), "draft.role_player_type")
        } else {
            resolve_named_type(
                &role.name,
                &[&role.why_involved, &role.expected_behavior],
                catalog,
            )
        };
        prepared.roles.push(RoleSpec {
            name: sanitize_identifier(&role.name),
            purpose: role.why_involved.clone(),
            expected_behavior: role.expected_behavior.clone(),
            type_status: role_type,
            methods: Vec::new(),
        });
    }
    for prop in &draft.props {
        let prop_type = resolve_named_type(&prop.name, &[&prop.meaning, &prop.notes], catalog);
        prepared.props.push(PropSpec {
            name: sanitize_identifier(&prop.name),
            meaning: prop.meaning.clone(),
            type_status: prop_type,
            notes: non_empty_lines(&prop.notes),
        });
    }

    for group in &draft.role_methods {
        let role_key = sanitize_identifier(&group.role);
        let Some(role_index) = prepared.roles.iter().position(|role| role.name == role_key) else {
            prepared.ambiguities.push(Ambiguity {
                path: format!("roles.{role_key}"),
                severity: "blocking".to_string(),
                message: format!(
                    "role method group `{}` does not map to a declared role",
                    group.role
                ),
                source_line: None,
            });
            continue;
        };
        let inferred_role_type = prepared.roles[role_index].type_status.clone();
        let methods = prepare_role_methods(group, &role_key, &inferred_role_type, catalog);
        for method in &methods {
            role_method_lookup
                .entry(role_key.clone())
                .or_default()
                .insert(method.name.clone());
        }
        prepared.roles[role_index].methods = methods;
    }

    let role_names: BTreeSet<String> = draft
        .roles
        .iter()
        .map(|role| sanitize_identifier(&role.name))
        .collect();
    let prop_names: BTreeSet<String> = draft
        .props
        .iter()
        .map(|prop| sanitize_identifier(&prop.name))
        .collect();
    let body_context = BodyParseContext {
        role_methods: role_method_lookup,
        role_names,
        prop_names,
    };
    for functionality in &draft.functionalities {
        prepared.functionalities.push(prepare_functionality(
            functionality,
            draft,
            &body_context,
            catalog,
        )?);
    }
    Ok(prepared)
}

fn prepare_role_methods(
    group: &RoleMethodGroup,
    role_key: &str,
    role_type: &ValueStatus,
    catalog: &DraftCatalog,
) -> Vec<MethodSpec> {
    group
        .methods
        .iter()
        .map(|method| {
            let evidence = vec![Evidence {
                section: format!("Role Methods / {}", group.role),
                text: method.detail.clone(),
            }];
            let parsed_signature = extract_signature_marker(&[method.detail.clone()])
                .and_then(|signature| parse_signature(&signature, &method.name).ok())
                .or_else(|| infer_getter_signature(role_type, &method.name, catalog));
            // The role player parameter `<role>_: &<RolePlayerType>` is only injected here when
            // the role type is already resolved. If it is unresolved, `propagate_resolved_types`
            // will inject it later (after the fix-agent or a manual edit supplies the type).
            // This avoids creating new blocking ambiguities that did not exist before this feature.
            let role_player_param: Option<ParameterSpec> =
                role_type.rust().map(|ty| ParameterSpec {
                    name: format!("{role_key}_"),
                    type_status: ValueStatus::resolved(format!("&{ty}"), "prepare.role_player"),
                });

            let (signature, receiver, mut parameters, return_status) = if let Some(parsed) =
                parsed_signature.clone()
            {
                let signature = ValueStatus::resolved(parsed.original.clone(), "prepare.signature");
                // Receiver is always `&self`; the context is never mutated by a role method call.
                let receiver = Some("&self".to_string());
                let parameters = parsed
                    .parameters
                    .iter()
                    .map(|parameter| ParameterSpec {
                        name: parameter.0.clone(),
                        type_status: ValueStatus::resolved(
                            parameter.1.clone(),
                            "prepare.signature",
                        ),
                    })
                    .collect::<Vec<_>>();
                let return_status =
                    ValueStatus::resolved(parsed.return_type.clone(), "prepare.signature");
                (signature, receiver, parameters, return_status)
            } else {
                (
                    ValueStatus::missing(
                        format!(
                            "role method `{}` on role `{}` is missing a parseable signature",
                            method.name, group.role
                        ),
                        evidence.clone(),
                    ),
                    Some("&self".to_string()),
                    Vec::new(),
                    ValueStatus::missing(
                        format!("role method `{}` return type is unknown", method.name),
                        evidence.clone(),
                    ),
                )
            };

            // Prepend the role player as the first explicit parameter when the type is resolved.
            if let Some(param) = role_player_param {
                parameters.insert(0, param);
            }

            let body = parsed_signature.map(|parsed| {
                delegated_role_method_body(role_key, &method.name, &parsed.parameters)
            });

            // Role methods in this implementation are instance methods on the context where
            // the first argument is the role player (named `<role>_` by convention).
            // If we have a delegated body there is no need for a behavioral description;
            // if the signature was not resolved we have nothing useful to store yet either,
            // so flow/extensions/guarantee stay empty for role methods.
            MethodSpec {
                name: sanitize_identifier(&method.name),
                signature,
                receiver,
                parameters,
                return_status,
                flow: Vec::new(),
                extensions: Vec::new(),
                guarantee: Vec::new(),
                references: None,
                body,
            }
        })
        .collect()
}

fn prepare_functionality(
    functionality: &FunctionalityDraft,
    draft: &CompositeDraft,
    body_context: &BodyParseContext,
    _catalog: &DraftCatalog,
) -> Result<MethodSpec> {
    if functionality.name == "new" {
        return Ok(auto_constructor_method(draft, _catalog));
    }

    let default_receiver = "&self";

    let signature_text = extract_signature_marker(&functionality.flow);
    let signature = if let Some(text) = signature_text.clone() {
        parse_signature(&text, &functionality.name).ok()
    } else {
        None
    };
    let evidence = vec![Evidence {
        section: format!("Functionalities / {}", functionality.name),
        text: functionality.flow.join(" "),
    }];
    let (signature_status, receiver, parameters, return_status) = if let Some(parsed) = &signature {
        (
            ValueStatus::resolved(parsed.original.clone(), "prepare.signature"),
            // Honour any receiver the BA put in the Signature marker; fall back to the type default.
            parsed
                .receiver
                .clone()
                .or_else(|| Some(default_receiver.to_string())),
            parsed
                .parameters
                .iter()
                .map(|parameter| ParameterSpec {
                    name: parameter.0.clone(),
                    type_status: ValueStatus::resolved(parameter.1.clone(), "prepare.signature"),
                })
                .collect::<Vec<_>>(),
            ValueStatus::resolved(parsed.return_type.clone(), "prepare.signature"),
        )
    } else {
        (
            ValueStatus::missing(
                format!(
                    "functionality `{}` is missing a parseable signature",
                    functionality.name
                ),
                evidence.clone(),
            ),
            Some(default_receiver.to_string()),
            Vec::new(),
            ValueStatus::missing(
                format!(
                    "functionality `{}` return type is unknown",
                    functionality.name
                ),
                evidence.clone(),
            ),
        )
    };

    // Dual-path: try to reduce to deterministic IR first; fall back to storing the
    // behavioral description verbatim so the AI implementation agent can use it.
    let body = parse_body_from_rules(&functionality.flow, body_context).ok();
    let (flow, extensions, guarantee, references) = if body.is_none() {
        let refs = extract_references(
            &functionality.flow,
            &functionality.extensions,
            &functionality.guarantee,
            body_context,
        );
        (
            functionality.flow.clone(),
            functionality.extensions.clone(),
            functionality.guarantee.clone(),
            Some(refs),
        )
    } else {
        (Vec::new(), Vec::new(), Vec::new(), None)
    };

    Ok(MethodSpec {
        name: sanitize_identifier(&functionality.name),
        signature: signature_status,
        receiver,
        parameters,
        return_status,
        flow,
        extensions,
        guarantee,
        references,
        body,
    })
}

fn auto_constructor_method(draft: &CompositeDraft, catalog: &DraftCatalog) -> MethodSpec {
    let mut params = Vec::new();
    let mut fields = Vec::new();
    for role in &draft.roles {
        let ty = if let Some(explicit) = &role.explicit_type {
            ValueStatus::resolved(explicit.clone(), "draft.role_player_type")
        } else {
            resolve_named_type(
                &role.name,
                &[&role.why_involved, &role.expected_behavior],
                catalog,
            )
        };
        params.push(ParameterSpec {
            name: sanitize_identifier(&role.name),
            type_status: ty.clone(),
        });
        fields.push(StructFieldValue {
            name: sanitize_identifier(&role.name),
            expr: Expression::Var {
                name: sanitize_identifier(&role.name),
            },
        });
    }
    for prop in &draft.props {
        let ty = resolve_named_type(&prop.name, &[&prop.meaning, &prop.notes], catalog);
        params.push(ParameterSpec {
            name: sanitize_identifier(&prop.name),
            type_status: ty.clone(),
        });
        fields.push(StructFieldValue {
            name: sanitize_identifier(&prop.name),
            expr: Expression::Var {
                name: sanitize_identifier(&prop.name),
            },
        });
    }

    let signature_text = format!(
        "new({}) -> Self",
        params
            .iter()
            .map(|parameter| format!(
                "{}: {}",
                parameter.name,
                parameter
                    .type_status
                    .rust
                    .clone()
                    .unwrap_or_else(|| "Unknown".to_string())
            ))
            .collect::<Vec<_>>()
            .join(", ")
    );

    MethodSpec {
        name: "new".to_string(),
        signature: ValueStatus::resolved(signature_text, "prepare.auto"),
        receiver: None,
        parameters: params,
        return_status: ValueStatus::resolved("Self", "prepare.auto"),
        flow: Vec::new(),
        extensions: Vec::new(),
        guarantee: Vec::new(),
        references: None,
        body: Some(Body {
            steps: vec![Statement::Return {
                expr: Some(Expression::ConstructStruct {
                    type_name: "Self".to_string(),
                    fields,
                }),
            }],
        }),
    }
}

fn prepare_app(app: &AppDraft, catalog: &DraftCatalog) -> Result<PreparedArtifact> {
    let export = export_name(app.info.title());
    // App entry points drive mutable state (main loop, startup sequence).
    let mut prepared = PreparedArtifact::empty(
        app.info.kind.as_str(),
        app.info.relative_path.clone(),
        app.info.title.clone(),
        export,
        true,
    );

    for collaborator in &app.collaborators {
        prepared.collaborators.push(CollaboratorSpec {
            name: sanitize_identifier(&collaborator.name),
            responsibility: collaborator.responsibility.clone(),
            type_status: resolve_named_type(
                &collaborator.name,
                &[&collaborator.responsibility],
                catalog,
            ),
        });
    }

    let mut rules = Vec::new();
    if let Some(body) = app.sections.get("Startup Sequence") {
        rules.extend(parse_loose_steps(body));
    }
    if let Some(body) = app.sections.get("Main Flow") {
        rules.extend(parse_loose_steps(body));
    } else if let Some(body) = app.sections.get("Main Loop Behavior") {
        rules.extend(parse_loose_steps(body));
    }

    let body_result = parse_body_from_rules(&rules, &BodyParseContext::default());
    let body_error = body_result.as_ref().err().map(|error| error.to_string());
    let body = body_result.ok();
    prepared.functionalities.push(MethodSpec {
        name: "main".to_string(),
        signature: ValueStatus::resolved("main() -> ()", "prepare.auto"),
        receiver: None,
        parameters: Vec::new(),
        return_status: ValueStatus::resolved("()", "prepare.auto"),
        flow: Vec::new(),
        extensions: Vec::new(),
        guarantee: Vec::new(),
        references: None,
        body,
    });

    if prepared
        .functionalities
        .first()
        .and_then(|method| method.body.as_ref())
        .is_none()
    {
        prepared.ambiguities.push(Ambiguity {
            path: "functionalities[0].body".to_string(),
            severity: "info".to_string(),
            message: format!(
                "app main flow could not be normalized into deterministic steps: {}",
                body_error.unwrap_or_else(|| "missing body".to_string())
            ),
            source_line: None,
        });
    }
    Ok(prepared)
}

/// Extract identifiers referenced in a method's behavioral description and classify them against
/// the DCI vocabulary known at prepare time (roles, props, role methods, types).
///
/// Scans backtick-quoted tokens in flow/extensions/guarantee. Each token is matched against:
///  - known role field names → `roles`
///  - known prop names → `props`
///  - known role methods (`<role>_<method>` where role is a known role field) → `role_methods`
///  - PascalCase identifiers → `types`
///
/// Identifiers that do not match any category are silently ignored (they are likely local
/// variable names or Rust expressions and are not meaningful at this level).
fn extract_references(
    flow: &[String],
    extensions: &[String],
    guarantee: &[String],
    body_context: &BodyParseContext,
) -> MethodReferences {
    let backtick_re = Regex::new(r"`([^`]+)`").expect("backtick regex");
    let mut refs = MethodReferences::default();

    let all_text = flow.iter().chain(extensions).chain(guarantee);
    for text in all_text {
        for cap in backtick_re.captures_iter(text) {
            let token = cap.get(1).unwrap().as_str().trim();
            // Strip method-call suffixes like `.method(...)` or `(args)` to isolate the
            // leading identifier (e.g. `stdin_source.read_keys()` → `stdin_source`).
            let identifier = token
                .split(|ch: char| ch == '.' || ch == '(')
                .next()
                .unwrap_or(token)
                .trim();
            let sanitized = sanitize_identifier(identifier);
            if sanitized.is_empty() {
                continue;
            }

            // 1. Exact role name (e.g. `board`, `stdin_source`).
            if body_context.role_names.contains(&sanitized) {
                if !refs.roles.contains(&sanitized) {
                    refs.roles.push(sanitized.clone());
                }
                continue;
            }

            // 2. Exact prop name.
            if body_context.prop_names.contains(&sanitized) {
                if !refs.props.contains(&sanitized) {
                    refs.props.push(sanitized.clone());
                }
                continue;
            }

            // 3. Role method: `<role>_<method>` where the role prefix is a known role name.
            let is_role_method = body_context.role_names.iter().any(|role| {
                sanitized.starts_with(&format!("{role}_")) && sanitized.len() > role.len() + 1
            });
            if is_role_method {
                if !refs.role_methods.contains(&sanitized) {
                    refs.role_methods.push(sanitized.clone());
                }
                continue;
            }

            // 4. PascalCase → treat as a type reference.
            if sanitized
                .chars()
                .next()
                .is_some_and(|ch| ch.is_ascii_uppercase())
            {
                if !refs.types.contains(&sanitized) {
                    refs.types.push(sanitized.clone());
                }
                continue;
            }

            // Everything else is assumed to be a local variable name or Rust keyword; skip.
        }
    }

    refs.roles.sort();
    refs.props.sort();
    refs.types.sort();
    refs.role_methods.sort();
    refs
}

fn parse_loose_steps(body: &str) -> Vec<String> {
    body.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                return None;
            }
            let trimmed = trimmed
                .strip_prefix("- ")
                .or_else(|| trimmed.strip_prefix("* "))
                .or_else(|| {
                    let digits = trimmed.chars().take_while(|ch| ch.is_ascii_digit()).count();
                    if digits > 0 && trimmed[digits..].starts_with(". ") {
                        Some(trimmed[digits + 2..].trim())
                    } else {
                        None
                    }
                })
                .unwrap_or(trimmed);
            Some(trimmed.to_string())
        })
        .collect()
}

fn parse_body_from_rules(rules: &[String], context: &BodyParseContext) -> Result<Body> {
    let mut steps = Vec::new();
    for rule in rules {
        if rule.trim().starts_with("Signature:") || rule.trim() == "Steps:" {
            continue;
        }
        steps.push(parse_step(rule, context)?);
    }
    if steps.is_empty() {
        anyhow::bail!("no executable steps found");
    }
    Ok(Body { steps })
}

fn parse_step(rule: &str, context: &BodyParseContext) -> Result<Statement> {
    if let Some(captures) = Regex::new(r#"^Let `([^`]+)` be `([^`]+)`\.?$"#)
        .expect("let regex")
        .captures(rule.trim())
    {
        return Ok(Statement::Let {
            name: sanitize_identifier(captures.get(1).unwrap().as_str()),
            expr: parse_expression(captures.get(2).unwrap().as_str(), context)?,
        });
    }
    if let Some(captures) = Regex::new(r#"^Set `([^`]+)` to `([^`]+)`\.?$"#)
        .expect("assign regex")
        .captures(rule.trim())
    {
        return Ok(Statement::AssignLocal {
            name: sanitize_identifier(captures.get(1).unwrap().as_str()),
            expr: parse_expression(captures.get(2).unwrap().as_str(), context)?,
        });
    }
    if let Some(captures) = Regex::new(r#"^Call `([^`]+)`\.?$"#)
        .expect("call regex")
        .captures(rule.trim())
    {
        return Ok(Statement::Call {
            expr: parse_expression(captures.get(1).unwrap().as_str(), context)?,
        });
    }
    if let Some(captures) = Regex::new(r#"^Return `([^`]+)`\.?$"#)
        .expect("return regex")
        .captures(rule.trim())
    {
        return Ok(Statement::Return {
            expr: Some(parse_expression(
                captures.get(1).unwrap().as_str(),
                context,
            )?),
        });
    }
    if let Some(captures) = Regex::new(r#"^Read current UTC millisecond time into `([^`]+)`\.?$"#)
        .expect("time regex")
        .captures(rule.trim())
    {
        return Ok(Statement::ReadUtcNowMs {
            name: sanitize_identifier(captures.get(1).unwrap().as_str()),
        });
    }
    if let Some(captures) = Regex::new(r#"^Sleep `([^`]+)` milliseconds\.?$"#)
        .expect("sleep regex")
        .captures(rule.trim())
    {
        return Ok(Statement::SleepMs {
            expr: parse_expression(captures.get(1).unwrap().as_str(), context)?,
        });
    }
    anyhow::bail!("Unsupported deterministic step: {rule}")
}

fn parse_expression(code: &str, context: &BodyParseContext) -> Result<Expression> {
    let trimmed = code.trim();
    if let Some((left, operator, right)) = find_top_level_binary_op(trimmed) {
        return Ok(Expression::BinaryOp {
            operator: operator.to_string(),
            left: Box::new(parse_expression(left, context)?),
            right: Box::new(parse_expression(right, context)?),
        });
    }
    if let Some(value) = trimmed.strip_prefix('!') {
        return Ok(Expression::UnaryOp {
            operator: "!".to_string(),
            expr: Box::new(parse_expression(value, context)?),
        });
    }
    if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 {
        return Ok(Expression::Literal {
            kind: "string".to_string(),
            value: trimmed[1..trimmed.len() - 1].to_string(),
        });
    }
    if trimmed == "true" || trimmed == "false" {
        return Ok(Expression::Literal {
            kind: "bool".to_string(),
            value: trimmed.to_string(),
        });
    }
    if trimmed.chars().all(|ch| ch.is_ascii_digit()) {
        return Ok(Expression::Literal {
            kind: "integer".to_string(),
            value: trimmed.to_string(),
        });
    }
    if let Some((type_name, fields)) = parse_struct_literal(trimmed, context)? {
        return Ok(Expression::ConstructStruct { type_name, fields });
    }
    if trimmed.contains("::")
        && !trimmed.contains('(')
        && trimmed.rsplit("::").next().is_some_and(|part| {
            part.chars()
                .next()
                .is_some_and(|ch| ch.is_ascii_uppercase())
        })
    {
        let type_name = trimmed.split("::").next().unwrap_or_default().to_string();
        let variant = trimmed.rsplit("::").next().unwrap_or_default().to_string();
        return Ok(Expression::ConstructEnum { type_name, variant });
    }
    if let Some((callee, args)) = parse_call(trimmed)? {
        if let Some((left, method)) = callee.rsplit_once('.') {
            if context
                .role_methods
                .get(&sanitize_identifier(left))
                .is_some_and(|methods| methods.contains(&sanitize_identifier(method)))
            {
                return Ok(Expression::CallRoleMethod {
                    role: sanitize_identifier(left),
                    method: sanitize_identifier(method),
                    args: parse_call_args(&args, context)?,
                });
            }
            return Ok(Expression::CallInstanceMethod {
                receiver: Box::new(parse_expression(left, context)?),
                method: sanitize_identifier(method.trim_end_matches('!')),
                args: parse_call_args(&args, context)?,
            });
        }
        return Ok(Expression::CallLocalMethod {
            name: callee.to_string(),
            args: parse_call_args(&args, context)?,
        });
    }
    if let Some((base, field)) = trimmed.rsplit_once('.') {
        return Ok(Expression::Field {
            base: sanitize_identifier(base),
            name: sanitize_identifier(field),
        });
    }
    Ok(Expression::Var {
        name: sanitize_identifier(trimmed),
    })
}

fn parse_call(code: &str) -> Result<Option<(String, String)>> {
    let Some(open) = code.find('(') else {
        return Ok(None);
    };
    if !code.ends_with(')') {
        anyhow::bail!("Unsupported call expression `{code}`");
    }
    Ok(Some((
        code[..open].trim().to_string(),
        code[open + 1..code.len() - 1].trim().to_string(),
    )))
}

fn parse_call_args(args: &str, context: &BodyParseContext) -> Result<Vec<Expression>> {
    if args.trim().is_empty() {
        return Ok(Vec::new());
    }
    split_top_level(args, ',')
        .into_iter()
        .map(|item| parse_expression(item.trim(), context))
        .collect()
}

fn parse_struct_literal(
    code: &str,
    context: &BodyParseContext,
) -> Result<Option<(String, Vec<StructFieldValue>)>> {
    let Some(open) = code.find('{') else {
        return Ok(None);
    };
    if !code.ends_with('}') {
        return Ok(None);
    }
    let type_name = code[..open].trim();
    if type_name.is_empty() {
        return Ok(None);
    }
    let inner = code[open + 1..code.len() - 1].trim();
    let mut fields = Vec::new();
    for part in split_top_level(inner, ',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let Some((name, value)) = part.split_once(':') else {
            anyhow::bail!("Unsupported struct field assignment `{part}`");
        };
        fields.push(StructFieldValue {
            name: sanitize_identifier(name),
            expr: parse_expression(value.trim(), context)?,
        });
    }
    Ok(Some((type_name.to_string(), fields)))
}

fn find_top_level_binary_op(code: &str) -> Option<(&str, &'static str, &str)> {
    for operator in ["==", "!=", ">=", "<=", "+", "-", "*", "/", ">", "<"] {
        let mut depth = 0i32;
        let chars = code.char_indices().collect::<Vec<_>>();
        for (idx, ch) in chars {
            match ch {
                '(' | '{' | '[' | '<' => depth += 1,
                ')' | '}' | ']' | '>' => depth -= 1,
                _ => {}
            }
            if depth == 0 && code[idx..].starts_with(operator) {
                let left = code[..idx].trim();
                let right = code[idx + operator.len()..].trim();
                if !left.is_empty() && !right.is_empty() {
                    return Some((left, operator, right));
                }
            }
        }
    }
    None
}

fn split_top_level(value: &str, delimiter: char) -> Vec<String> {
    let mut items = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (idx, ch) in value.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
                continue;
            }
            match ch {
                '\\' => {
                    escaped = true;
                    continue;
                }
                '"' => {
                    in_string = false;
                    continue;
                }
                _ => continue,
            }
        }
        if ch == '"' {
            in_string = true;
            continue;
        }
        match ch {
            '(' | '{' | '[' | '<' => depth += 1,
            ')' | '}' | ']' | '>' => depth -= 1,
            _ => {}
        }
        if depth == 0 && ch == delimiter {
            items.push(value[start..idx].trim().to_string());
            start = idx + ch.len_utf8();
        }
    }
    items.push(value[start..].trim().to_string());
    items
}

fn delegated_role_method_body(role: &str, method: &str, parameters: &[(String, String)]) -> Body {
    // The role player is received as the first parameter named `<role>_`.
    // Delegate to the role player's own method, passing the remaining domain parameters.
    let args = parameters
        .iter()
        .map(|parameter| Expression::Var {
            name: sanitize_identifier(&parameter.0),
        })
        .collect::<Vec<_>>();
    Body {
        steps: vec![Statement::Return {
            expr: Some(Expression::CallInstanceMethod {
                receiver: Box::new(Expression::Var {
                    name: format!("{role}_"),
                }),
                method: sanitize_identifier(method),
                args,
            }),
        }],
    }
}

#[derive(Debug, Clone)]
struct ParsedSignature {
    original: String,
    receiver: Option<String>,
    parameters: Vec<(String, String)>,
    return_type: String,
}

fn parse_signature(signature: &str, expected_name: &str) -> Result<ParsedSignature> {
    let trimmed = strip_backticks(signature);
    let Some((left, right)) = trimmed.split_once("->") else {
        anyhow::bail!("signature `{trimmed}` is missing `->`");
    };
    let left = left.trim();
    let return_type = right.trim().to_string();
    let Some(open) = left.find('(') else {
        anyhow::bail!("signature `{trimmed}` is missing `(`");
    };
    let Some(close) = left.rfind(')') else {
        anyhow::bail!("signature `{trimmed}` is missing `)`");
    };
    let name = left[..open].trim();
    if sanitize_identifier(name) != sanitize_identifier(expected_name) {
        anyhow::bail!("signature `{trimmed}` describes `{name}` but expected `{expected_name}`");
    }
    let inner = left[open + 1..close].trim();
    let mut receiver = None;
    let mut parameters = Vec::new();
    for part in split_top_level(inner, ',') {
        if part.is_empty() {
            continue;
        }
        let part = part.trim();
        if matches!(part, "&self" | "&mut self" | "self") {
            receiver = Some(part.to_string());
            continue;
        }
        let Some((name, ty)) = part.split_once(':') else {
            anyhow::bail!("signature parameter `{part}` is missing `:`");
        };
        parameters.push((sanitize_identifier(name), ty.trim().to_string()));
    }
    Ok(ParsedSignature {
        original: trimmed,
        receiver,
        parameters,
        return_type,
    })
}

fn extract_signature_marker(lines: &[String]) -> Option<String> {
    let regex = Regex::new(r#"Signature:\s*`([^`]+)`"#).expect("signature regex");
    let mut matches = lines
        .iter()
        .filter_map(|line| regex.captures(line).and_then(|caps| caps.get(1)))
        .map(|value| value.as_str().trim().to_string())
        .collect::<Vec<_>>();
    matches.dedup();
    matches.into_iter().next()
}

fn infer_getter_signature(
    role_type: &ValueStatus,
    method_name: &str,
    catalog: &DraftCatalog,
) -> Option<ParsedSignature> {
    let export = role_type.rust()?;
    let data = catalog.data_drafts.get(export)?;
    let field = data
        .fields
        .iter()
        .find(|field| sanitize_identifier(&field.name) == sanitize_identifier(method_name))?;
    let field_type = resolve_data_field_type(
        field.name.as_str(),
        &[&field.meaning, &field.notes],
        catalog,
    );
    let return_type = if copy_like_type(field_type.rust()) {
        field_type.rust()?.to_string()
    } else {
        format!("&{}", field_type.rust()?)
    };
    Some(ParsedSignature {
        original: format!(
            "{}(&self) -> {}",
            sanitize_identifier(method_name),
            return_type
        ),
        receiver: Some("&self".to_string()),
        parameters: Vec::new(),
        return_type,
    })
}

fn resolve_data_field_type(name: &str, texts: &[&str], catalog: &DraftCatalog) -> ValueStatus {
    resolve_type_with_defaults(name, texts, catalog, true, false)
}

fn resolve_named_type(name: &str, texts: &[&str], catalog: &DraftCatalog) -> ValueStatus {
    resolve_type_with_defaults(name, texts, catalog, false, true)
}

fn resolve_type_with_defaults(
    name: &str,
    texts: &[&str],
    catalog: &DraftCatalog,
    allow_scalar_defaults: bool,
    prefer_exact_name_match: bool,
) -> ValueStatus {
    let evidence = texts
        .iter()
        .filter(|text| !text.trim().is_empty())
        .map(|text| Evidence {
            section: name.to_string(),
            text: text.trim().to_string(),
        })
        .collect::<Vec<_>>();

    if prefer_exact_name_match {
        let normalized_name = normalize_symbol(name);
        if let Some(candidates) = catalog.normalized.get(&normalized_name) {
            if candidates.len() == 1 {
                return ValueStatus::resolved(candidates[0].clone(), "name_match");
            }
            return ValueStatus::ambiguous(
                candidates.clone(),
                format!("multiple exports match `{name}`"),
                evidence,
            );
        }
    }

    let explicit = extract_type_candidates(texts, catalog);
    if explicit.len() == 1 {
        return ValueStatus::resolved(explicit[0].clone(), "draft_hint");
    }
    if explicit.len() > 1 {
        return ValueStatus::ambiguous(
            explicit,
            format!("multiple type candidates were found for `{name}`"),
            evidence,
        );
    }

    if !prefer_exact_name_match {
        let normalized_name = normalize_symbol(name);
        if let Some(candidates) = catalog.normalized.get(&normalized_name) {
            if candidates.len() == 1 {
                return ValueStatus::resolved(candidates[0].clone(), "name_match");
            }
            return ValueStatus::ambiguous(
                candidates.clone(),
                format!("multiple exports match `{name}`"),
                evidence,
            );
        }
    }

    if allow_scalar_defaults {
        let lower = format!("{} {}", name, texts.join(" ")).to_ascii_lowercase();
        if lower.contains("string") || lower.contains("text") {
            return ValueStatus::defaulted("String", "scalar_default");
        }
        if lower.contains("millisecond") || lower.ends_with("_ms") {
            return ValueStatus::defaulted("u64", "scalar_default");
        }
        if lower.contains("whole number") || lower.contains("count") || lower.contains("score") {
            return ValueStatus::defaulted("u32", "scalar_default");
        }
        if lower.contains("true") || lower.contains("false") || lower.contains("boolean") {
            return ValueStatus::defaulted("bool", "scalar_default");
        }
    }

    ValueStatus::missing(
        format!("no concrete Rust type could be inferred for `{name}`"),
        evidence,
    )
}

fn extract_type_candidates(texts: &[&str], catalog: &DraftCatalog) -> Vec<String> {
    let backtick = Regex::new(r"`([^`]+)`").expect("type hint regex");
    let explicit_prefix =
        Regex::new(r"(?i)\btype is\s+`?([A-Za-z0-9_:<>, &\[\]]+)`?").expect("explicit type regex");
    let mut candidates = BTreeSet::new();
    for text in texts {
        for captures in explicit_prefix.captures_iter(text) {
            let raw = captures.get(1).unwrap().as_str().trim().to_string();
            if looks_like_type(&raw, catalog) {
                candidates.insert(raw);
            }
        }
        for captures in backtick.captures_iter(text) {
            let raw = captures.get(1).unwrap().as_str().trim().to_string();
            if looks_like_type(&raw, catalog) {
                candidates.insert(raw);
            }
        }
    }
    candidates.into_iter().collect()
}

fn looks_like_type(value: &str, catalog: &DraftCatalog) -> bool {
    let trimmed = strip_backticks(value);
    if trimmed.is_empty() {
        return false;
    }
    if matches!(trimmed.as_str(), "&self" | "&mut self" | "self") {
        return false;
    }
    if trimmed.contains(" -> ") || trimmed.contains('(') || trimmed.contains(')') {
        return false;
    }
    if catalog.exports.contains(&trimmed) {
        return true;
    }
    if trimmed.contains("::") || trimmed.contains('<') || trimmed.contains('[') {
        return true;
    }
    trimmed
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_uppercase())
        || matches!(
            trimmed.as_str(),
            "String"
                | "str"
                | "bool"
                | "u8"
                | "u16"
                | "u32"
                | "u64"
                | "usize"
                | "i8"
                | "i16"
                | "i32"
                | "i64"
                | "isize"
                | "char"
                | "()"
        )
}

fn infer_derives(notes: &[String]) -> Vec<String> {
    let mut derives = vec!["Debug".to_string(), "Clone".to_string()];
    let lower = notes.join(" ").to_ascii_lowercase();
    if lower.contains("copy") {
        derives.push("Copy".to_string());
    }
    derives.sort();
    derives.dedup();
    derives
}

fn getter_mode(rust: Option<&str>) -> &'static str {
    if copy_like_type(rust) { "copy" } else { "ref" }
}

fn copy_like_type(rust: Option<&str>) -> bool {
    matches!(
        rust.unwrap_or(""),
        "bool"
            | "char"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "usize"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "isize"
            | "f32"
            | "f64"
    )
}

fn export_name(value: &str) -> String {
    let mut out = String::new();
    for token in value
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
    {
        let mut chars = token.chars();
        if let Some(first) = chars.next() {
            out.push_str(&first.to_uppercase().collect::<String>());
            out.push_str(chars.as_str());
        }
    }
    if out.is_empty() {
        "Artifact".to_string()
    } else {
        out
    }
}

fn sanitize_identifier(value: &str) -> String {
    let mut out = String::new();
    let mut prev_underscore = false;
    for ch in value.chars() {
        let normalized = if ch.is_ascii_alphanumeric() {
            ch.to_ascii_lowercase()
        } else {
            '_'
        };
        if normalized == '_' {
            if !prev_underscore && !out.is_empty() {
                out.push('_');
            }
            prev_underscore = true;
            continue;
        }
        prev_underscore = false;
        out.push(normalized);
    }
    out.trim_matches('_').to_string()
}

fn normalize_symbol(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(|ch| ch.to_lowercase())
        .collect()
}

fn non_empty_lines(value: &str) -> Vec<String> {
    value
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn strip_backticks(value: &str) -> String {
    value.trim().trim_matches('`').to_string()
}

trait DraftInfoExt {
    fn relative_path(&self) -> &str;
    fn title(&self) -> &str;
    fn stem(&self) -> &str;
}

impl DraftInfoExt for crate::draft_parser::DraftInfo {
    fn relative_path(&self) -> &str {
        &self.relative_path
    }

    fn title(&self) -> &str {
        &self.title
    }

    fn stem(&self) -> &str {
        self.relative_path
            .rsplit('/')
            .next()
            .unwrap_or(&self.relative_path)
            .trim_end_matches(".md")
    }
}

trait DraftDocumentInfoExt {
    fn info(&self) -> &crate::draft_parser::DraftInfo;
}

impl DraftDocumentInfoExt for DraftDocument {
    fn info(&self) -> &crate::draft_parser::DraftInfo {
        match self {
            DraftDocument::Data(data) => &data.info,
            DraftDocument::Projection(projection) => &projection.info,
            DraftDocument::Context(context) => &context.info,
            DraftDocument::App(app) => &app.info,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_catalog(exports: &[&str]) -> DraftCatalog {
        let mut export_set = BTreeSet::new();
        let mut normalized = BTreeMap::new();
        for export in exports {
            let export = export.to_string();
            export_set.insert(export.clone());
            normalized.insert(normalize_symbol(&export), vec![export]);
        }
        DraftCatalog {
            exports: export_set,
            normalized,
            data_drafts: BTreeMap::new(),
        }
    }

    #[test]
    fn resolve_named_type_prefers_exact_name_match_over_incidental_hint() {
        let catalog = test_catalog(&["TerminalRenderer", "StringRenderer"]);
        let status = resolve_named_type(
            "TerminalRenderer",
            &["Uses `StringRenderer` and shows the latest frame in the terminal."],
            &catalog,
        );
        assert_eq!(status.rust.as_deref(), Some("TerminalRenderer"));
        assert_eq!(status.source.as_deref(), Some("name_match"));
    }

    #[test]
    fn available_types_for_app_uses_manifest_allowlists_and_prefixes() {
        let catalog = test_catalog(&["GameLoopContext"]);
        let manifest = TypesManifest {
            primitives: vec!["u32".to_string()],
            external_path_prefixes: vec!["std::".to_string()],
            allowlists: ManifestAllowlists {
                data: vec!["Board".to_string()],
                projection: vec!["StringRenderer".to_string()],
                context: vec!["TerminalRenderer".to_string()],
            },
        };

        let available = available_types_for_draft(
            Some(&manifest),
            crate::draft_parser::ArtifactKind::App,
            &catalog,
        );

        assert!(available.contains(&"GameLoopContext".to_string()));
        assert!(available.contains(&"Board".to_string()));
        assert!(available.contains(&"StringRenderer".to_string()));
        assert!(available.contains(&"TerminalRenderer".to_string()));
        assert!(available.contains(&"u32".to_string()));
        assert!(available.contains(&"std::".to_string()));
    }
}
