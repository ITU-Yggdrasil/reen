use anyhow::{Result, bail};
use regex::Regex;
use std::collections::BTreeMap;
use std::path::{Component, Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ArtifactKind {
    Data,
    Projection,
    Context,
    App,
    UnsupportedApi,
}

impl ArtifactKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ArtifactKind::Data => "data",
            ArtifactKind::Projection => "projection",
            ArtifactKind::Context => "context",
            ArtifactKind::App => "app",
            ArtifactKind::UnsupportedApi => "api",
        }
    }

    pub fn from_draft_path(relative_path: &Path) -> Option<Self> {
        match first_component(relative_path)? {
            "data" => Some(Self::Data),
            "projections" => Some(Self::Projection),
            "contexts" => Some(Self::Context),
            "apis" | "external_apis" => Some(Self::UnsupportedApi),
            "app.md" => Some(Self::App),
            _ => {
                if relative_path == Path::new("app.md") {
                    Some(Self::App)
                } else {
                    None
                }
            }
        }
    }

    pub fn from_prepared_path(relative_path: &Path) -> Option<Self> {
        match first_component(relative_path)? {
            "data" => Some(Self::Data),
            "projections" => Some(Self::Projection),
            "contexts" => Some(Self::Context),
            "app.yml" => Some(Self::App),
            _ => {
                if relative_path == Path::new("app.yml") {
                    Some(Self::App)
                } else {
                    None
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftInfo {
    pub kind: ArtifactKind,
    pub relative_path: String,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataField {
    pub name: String,
    pub meaning: String,
    pub notes: String,
    pub getter_accessible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataVariant {
    pub name: String,
    pub meaning: String,
    pub notes: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RolePlayer {
    pub name: String,
    pub why_involved: String,
    pub expected_behavior: String,
    /// Explicitly declared Rust type for this role player, already normalised from the draft's
    /// optional fourth column (accepts Rust path syntax or English descriptions).
    /// When `Some`, `prepare` uses this directly instead of inferring from context.
    pub explicit_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleMethod {
    pub name: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleMethodGroup {
    pub role: String,
    pub methods: Vec<RoleMethod>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropSpec {
    pub name: String,
    pub meaning: String,
    pub notes: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InteractionRow {
    pub started_by: String,
    pub uses: String,
    pub result: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExampleRow {
    pub given: String,
    pub when: String,
    pub then: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionalityDraft {
    pub name: String,
    pub interactions: Vec<InteractionRow>,
    /// Numbered main-flow steps, stripped of their leading `N. ` prefix.
    pub flow: Vec<String>,
    /// Alternative-path entries (keyed by step, e.g. `1a. …`), stripped of their `- ` prefix.
    pub extensions: Vec<String>,
    /// Post-condition invariants, either from a single inline line or from a bullet list.
    pub guarantee: Vec<String>,
    pub examples: Vec<ExampleRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollaboratorDraft {
    pub name: String,
    pub responsibility: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataDraft {
    pub info: DraftInfo,
    pub description: String,
    pub fields: Vec<DataField>,
    pub variants: Vec<DataVariant>,
    pub rules: Vec<String>,
    pub construction_rules: Vec<String>,
    pub access_rules: Vec<String>,
    pub functionality_sections: Vec<Section>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompositeDraft {
    pub info: DraftInfo,
    pub purpose: String,
    pub roles: Vec<RolePlayer>,
    pub role_methods: Vec<RoleMethodGroup>,
    pub props: Vec<PropSpec>,
    pub functionalities: Vec<FunctionalityDraft>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppDraft {
    pub info: DraftInfo,
    pub application_kind: Option<String>,
    pub sections: BTreeMap<String, String>,
    pub collaborators: Vec<CollaboratorDraft>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DraftDocument {
    Data(DataDraft),
    Projection(CompositeDraft),
    Context(CompositeDraft),
    App(AppDraft),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub title: String,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MarkdownTable {
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Subsection {
    title: String,
    body: String,
}

pub fn parse_draft_file(path: &Path, drafts_root: &Path, content: &str) -> Result<DraftDocument> {
    let relative_path = path
        .strip_prefix(drafts_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    let kind = ArtifactKind::from_draft_path(Path::new(&relative_path))
        .ok_or_else(|| anyhow::anyhow!("Unsupported draft path {}", relative_path))?;
    if kind == ArtifactKind::UnsupportedApi {
        bail!(
            "Unsupported draft scope under {}. V1 does not support drafts/apis or drafts/external_apis.",
            relative_path
        );
    }
    let title = extract_title(content).ok_or_else(|| {
        anyhow::anyhow!(
            "Draft schema validation failed for {}:\n- Missing `# <Title>` heading",
            relative_path
        )
    })?;
    let info = DraftInfo {
        kind,
        relative_path,
        title,
    };
    match kind {
        ArtifactKind::Data => parse_data_draft(info, content).map(DraftDocument::Data),
        ArtifactKind::Projection => {
            parse_composite_draft(info, content).map(DraftDocument::Projection)
        }
        ArtifactKind::Context => parse_composite_draft(info, content).map(DraftDocument::Context),
        ArtifactKind::App => parse_app_draft(info, content).map(DraftDocument::App),
        ArtifactKind::UnsupportedApi => unreachable!(),
    }
}

fn parse_data_draft(info: DraftInfo, content: &str) -> Result<DataDraft> {
    let sections = parse_sections(content);
    require_section(&info.relative_path, &sections, "Description")?;
    let description = section_text(&sections, "Description");
    let fields = if let Some(section) = find_section(&sections, "Fields") {
        parse_fields_table(&info.relative_path, &section.body)?
    } else {
        Vec::new()
    };
    let variants = if let Some(section) = find_section(&sections, "Variants") {
        parse_variants_table(&info.relative_path, &section.body)?
    } else {
        Vec::new()
    };
    if fields.is_empty() == variants.is_empty() {
        bail!(
            "Draft schema validation failed for {}:\n- Data drafts must contain exactly one of `## Fields` or `## Variants`",
            info.relative_path
        );
    }

    Ok(DataDraft {
        info,
        description,
        fields,
        variants,
        rules: bullet_lines(find_section(&sections, "Rules").map(|section| section.body.as_str())),
        construction_rules: bullet_lines(
            find_section(&sections, "Construction Rules").map(|section| section.body.as_str()),
        ),
        access_rules: bullet_lines(
            find_section(&sections, "Access Rules").map(|section| section.body.as_str()),
        ),
        functionality_sections: find_section(&sections, "Functionalities")
            .map(|section| parse_subsections(&section.body))
            .unwrap_or_default()
            .into_iter()
            .map(|subsection| Section {
                title: subsection.title,
                body: subsection.body,
            })
            .collect(),
        notes: bullet_lines(find_section(&sections, "Notes").map(|section| section.body.as_str())),
    })
}

fn parse_composite_draft(info: DraftInfo, content: &str) -> Result<CompositeDraft> {
    let sections = parse_sections(content);
    let purpose = section_text(&sections, "Purpose");
    if purpose.is_empty() {
        bail!(
            "Draft schema validation failed for {}:\n- Missing required `## Purpose` section",
            info.relative_path
        );
    }
    let roles = parse_role_players_section(&info.relative_path, &sections)?;
    let props_table =
        parse_single_table_section(&info.relative_path, &sections, "Props", &["Prop", "Meaning", "Notes"])?;
    let role_methods = parse_role_methods(
        &info.relative_path,
        &section_body(&sections, "Role Methods"),
    )?;
    let functionalities = parse_functionalities(
        &info.relative_path,
        &section_body(&sections, "Functionalities"),
    )?;

    Ok(CompositeDraft {
        info,
        purpose,
        roles,
        role_methods,
        props: props_table
            .rows
            .into_iter()
            .map(|row| PropSpec {
                name: row[0].clone(),
                meaning: row[1].clone(),
                notes: row[2].clone(),
            })
            .collect(),
        functionalities,
        notes: bullet_lines(find_section(&sections, "Notes").map(|section| section.body.as_str())),
    })
}

fn parse_app_draft(info: DraftInfo, content: &str) -> Result<AppDraft> {
    let sections = parse_sections(content);
    let mut rendered = BTreeMap::new();
    for section in &sections {
        rendered.insert(section.title.clone(), section.body.trim().to_string());
    }
    let application_kind = find_section(&sections, "Application Kind")
        .map(|section| strip_code_ticks(section.body.trim()))
        .filter(|value| !value.is_empty());
    let collaborators = if let Some(section) = find_section(&sections, "Collaborators and Wiring") {
        parse_collaborators_table(&info.relative_path, &section.body)?
    } else {
        Vec::new()
    };
    Ok(AppDraft {
        info,
        application_kind,
        sections: rendered,
        collaborators,
    })
}

fn parse_role_methods(relative_path: &str, body: &str) -> Result<Vec<RoleMethodGroup>> {
    if body.trim().is_empty() {
        bail!(
            "Draft schema validation failed for {}:\n- Missing required `## Role Methods` section",
            relative_path
        );
    }
    let subsections = parse_subsections(body);
    if subsections.is_empty() {
        bail!(
            "Draft schema validation failed for {}:\n- `## Role Methods` must use `### <Role Name>` subsections",
            relative_path
        );
    }
    Ok(subsections
        .into_iter()
        .map(|subsection| RoleMethodGroup {
            role: subsection.title,
            methods: parse_named_bullets(&subsection.body)
                .into_iter()
                .map(|(name, detail)| RoleMethod { name, detail })
                .collect(),
        })
        .collect())
}

fn parse_functionalities(relative_path: &str, body: &str) -> Result<Vec<FunctionalityDraft>> {
    if body.trim().is_empty() {
        bail!(
            "Draft schema validation failed for {}:\n- Missing required `## Functionalities` section",
            relative_path
        );
    }
    let subsections = parse_subsections(body);
    if subsections.is_empty() {
        bail!(
            "Draft schema validation failed for {}:\n- `## Functionalities` must contain `###` subsections",
            relative_path
        );
    }
    let mut items = Vec::new();
    for subsection in subsections {
        let tables = parse_tables(&subsection.body);
        let interaction = tables
            .iter()
            .find(|table| table.headers == ["Started by", "Uses", "Result"])
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Draft schema validation failed for {}:\n- Functionality `### {}` is missing the `Started by | Uses | Result` table",
                    relative_path,
                    subsection.title
                )
            })?;
        let examples = tables
            .iter()
            .find(|table| table.headers == ["Given", "When", "Then"])
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Draft schema validation failed for {}:\n- Functionality `### {}` is missing the `Given | When | Then` table",
                    relative_path,
                    subsection.title
                )
            })?;
        let flow = parse_flow_block(&subsection.body);
        if flow.is_empty() {
            bail!(
                "Draft schema validation failed for {}:\n- Functionality `### {}` must contain a `**Flow:**` numbered list",
                relative_path,
                subsection.title
            );
        }
        let extensions = parse_labeled_items_block(&subsection.body, "Extensions");
        let guarantee = parse_labeled_items_block(&subsection.body, "Guarantee");
        items.push(FunctionalityDraft {
            name: subsection.title,
            interactions: interaction
                .rows
                .iter()
                .map(|row| InteractionRow {
                    started_by: row[0].clone(),
                    uses: row[1].clone(),
                    result: row[2].clone(),
                })
                .collect(),
            flow,
            extensions,
            guarantee,
            examples: examples
                .rows
                .iter()
                .map(|row| ExampleRow {
                    given: row[0].clone(),
                    when: row[1].clone(),
                    then: row[2].clone(),
                })
                .collect(),
        });
    }
    Ok(items)
}

fn parse_fields_table(relative_path: &str, body: &str) -> Result<Vec<DataField>> {
    let tables = parse_tables(body);
    if tables.len() != 1 {
        bail!(
            "Draft schema validation failed for {}:\n- `## Fields` must contain exactly one markdown table",
            relative_path
        );
    }
    let table = &tables[0];
    let allowed_three = ["Field", "Meaning", "Notes"];
    let allowed_four = ["Field", "Meaning", "Accessible", "Notes"];
    if table.headers != allowed_three && table.headers != allowed_four {
        bail!(
            "Draft schema validation failed for {}:\n- `## Fields` must use `Field | Meaning | Notes` or `Field | Meaning | Accessible | Notes`",
            relative_path
        );
    }

    let has_accessible = table.headers == allowed_four;
    let mut rows = Vec::new();
    for row in &table.rows {
        let (getter_accessible, notes) = if has_accessible {
            let value = row[2].trim();
            let getter_accessible = value.is_empty()
                || matches_ignore_ascii_case(value, &["x", "yes", "true"]);
            (getter_accessible, row[3].clone())
        } else {
            (true, row[2].clone())
        };
        rows.push(DataField {
            name: row[0].clone(),
            meaning: row[1].clone(),
            notes,
            getter_accessible,
        });
    }
    Ok(rows)
}

fn parse_variants_table(relative_path: &str, body: &str) -> Result<Vec<DataVariant>> {
    let tables = parse_tables(body);
    if tables.len() != 1 {
        bail!(
            "Draft schema validation failed for {}:\n- `## Variants` must contain exactly one markdown table",
            relative_path
        );
    }
    let table = &tables[0];
    if table.headers != ["Variant", "Meaning", "Notes"] {
        bail!(
            "Draft schema validation failed for {}:\n- `## Variants` must use `Variant | Meaning | Notes`",
            relative_path
        );
    }
    Ok(table
        .rows
        .iter()
        .map(|row| DataVariant {
            name: row[0].clone(),
            meaning: row[1].clone(),
            notes: row[2].clone(),
        })
        .collect())
}

fn parse_collaborators_table(relative_path: &str, body: &str) -> Result<Vec<CollaboratorDraft>> {
    let tables = parse_tables(body);
    if tables.len() != 1 {
        bail!(
            "Draft schema validation failed for {}:\n- `## Collaborators and Wiring` must contain exactly one markdown table",
            relative_path
        );
    }
    let table = &tables[0];
    if table.headers != ["Collaborator", "Responsibility"] {
        bail!(
            "Draft schema validation failed for {}:\n- `## Collaborators and Wiring` must use `Collaborator | Responsibility`",
            relative_path
        );
    }
    Ok(table
        .rows
        .iter()
        .map(|row| CollaboratorDraft {
            name: strip_code_ticks(&row[0]),
            responsibility: row[1].clone(),
        })
        .collect())
}

fn parse_role_players_section(relative_path: &str, sections: &[Section]) -> Result<Vec<RolePlayer>> {
    let section = require_section(relative_path, sections, "Role Players")?;
    if section.body.trim().is_empty() {
        return Ok(Vec::new());
    }
    let tables = parse_tables(&section.body);
    if tables.len() != 1 {
        bail!(
            "Draft schema validation failed for {}:\n- `## Role Players` must contain exactly one markdown table",
            relative_path
        );
    }
    let table = &tables[0];
    let three_col = ["Role player", "Why involved", "Expected behaviour"];
    let four_col = ["Role player", "Why involved", "Expected behaviour", "Type"];
    let has_type_col = if table.headers == three_col.as_slice() {
        false
    } else if table.headers == four_col.as_slice() {
        true
    } else {
        bail!(
            "Draft schema validation failed for {}:\n- `## Role Players` must use headers \
             `Role player | Why involved | Expected behaviour` (optionally with a trailing `Type` column)",
            relative_path
        );
    };
    let mut roles = Vec::new();
    for row in &table.rows {
        let explicit_type = if has_type_col {
            let raw = row[3].trim();
            if raw.is_empty() {
                None
            } else {
                Some(normalize_type_notation(raw))
            }
        } else {
            None
        };
        roles.push(RolePlayer {
            name: row[0].clone(),
            why_involved: row[1].clone(),
            expected_behavior: row[2].clone(),
            explicit_type,
        });
    }
    Ok(roles)
}

/// Normalise a type string from the draft's optional `Type` column into a Rust type.
///
/// Accepts:
/// - Rust path syntax as-is (`std::io::Stdin`, `Vec<u8>`, `u64`, …)
/// - Backtick-quoted Rust types (stripped)
/// - English descriptions mapped deterministically to Rust types:
///   `integer` → `i64`, `string` / `text` → `String`, `list of X` → `Vec<X>`, …
pub fn normalize_type_notation(raw: &str) -> String {
    let s = raw.trim();
    // Strip outer backticks.
    let s = if s.starts_with('`') && s.ends_with('`') && s.len() >= 2 {
        &s[1..s.len() - 1]
    } else {
        s
    };
    let s = s.trim();

    // If it looks like a valid Rust type already (no lowercase words separated by spaces,
    // contains `::`, `<`, or is a known primitive), pass it through directly.
    if is_rust_type_syntax(s) {
        return s.to_string();
    }

    // Lower-case for English matching.
    let lower = s.to_ascii_lowercase();

    // --- Composite English patterns (recursive) ---

    // "list of X" → Vec<X>
    if let Some(inner) = lower.strip_prefix("list of ").or_else(|| lower.strip_prefix("array of ").or_else(|| lower.strip_prefix("sequence of ").or_else(|| lower.strip_prefix("vec of ")))) {
        let inner_ty = normalize_type_notation(inner);
        return format!("Vec<{inner_ty}>");
    }
    // "optional X" / "option of X" / "maybe X"
    if let Some(inner) = lower.strip_prefix("optional ").or_else(|| lower.strip_prefix("option of ").or_else(|| lower.strip_prefix("maybe "))) {
        let inner_ty = normalize_type_notation(inner);
        return format!("Option<{inner_ty}>");
    }
    // "map from X to Y" / "map of X to Y" / "mapping from X to Y"
    if let Some(rest) = lower.strip_prefix("map from ").or_else(|| lower.strip_prefix("map of ").or_else(|| lower.strip_prefix("mapping from "))) {
        if let Some(idx) = rest.find(" to ") {
            let key_ty = normalize_type_notation(&rest[..idx]);
            let val_ty = normalize_type_notation(&rest[idx + 4..]);
            return format!("std::collections::HashMap<{key_ty}, {val_ty}>");
        }
    }

    // --- English primitives ---
    match lower.as_str() {
        "integer" | "int" | "signed integer" | "whole number" => "i64".to_string(),
        "unsigned integer" | "unsigned int" | "natural number" | "count" => "u64".to_string(),
        "small integer" | "i32" => "i32".to_string(),
        "small unsigned integer" | "u32" => "u32".to_string(),
        "byte" | "u8" => "u8".to_string(),
        "float" | "floating point" | "rational number" | "decimal" | "real number" | "f64" => "f64".to_string(),
        "single precision float" | "f32" => "f32".to_string(),
        "string" | "text" | "str" => "String".to_string(),
        "boolean" | "bool" | "flag" => "bool".to_string(),
        "character" | "char" => "char".to_string(),
        "unit" | "()" => "()".to_string(),
        "duration" | "milliseconds" | "ms" => "u64".to_string(),
        "timestamp" | "unix time" | "epoch ms" => "u64".to_string(),
        _ => {
            // Fall back: return the original capitalised as a PascalCase type name,
            // which keeps it visible for the resolver rather than silently mangling it.
            s.to_string()
        }
    }
}

/// Returns true if the string looks like valid Rust type syntax (not English prose).
/// Heuristic: no whitespace (except inside `<>`), or contains `::`, or is a known primitive.
fn is_rust_type_syntax(s: &str) -> bool {
    // Known Rust primitive type names.
    let primitives = [
        "i8", "i16", "i32", "i64", "i128", "isize",
        "u8", "u16", "u32", "u64", "u128", "usize",
        "f32", "f64", "bool", "char", "str",
    ];
    if primitives.contains(&s) {
        return true;
    }
    // Contains `::` → qualified path.
    if s.contains("::") {
        return true;
    }
    // No ASCII spaces at the top level (outside angle brackets) → looks like a type expression.
    let mut depth = 0i32;
    for ch in s.chars() {
        match ch {
            '<' => depth += 1,
            '>' => depth -= 1,
            ' ' if depth == 0 => return false,
            _ => {}
        }
    }
    // PascalCase or known standard types.
    s.chars().next().is_some_and(|ch| ch.is_ascii_uppercase() || ch == '&' || ch == '(' || ch == '[')
}

fn parse_single_table_section(
    relative_path: &str,
    sections: &[Section],
    section_title: &str,
    expected_headers: &[&str],
) -> Result<MarkdownTable> {
    let section = require_section(relative_path, sections, section_title)?;
    if section.body.trim().is_empty() {
        return Ok(MarkdownTable {
            headers: expected_headers
                .iter()
                .map(|value| value.to_string())
                .collect(),
            rows: Vec::new(),
        });
    }
    let tables = parse_tables(&section.body);
    if tables.len() != 1 {
        bail!(
            "Draft schema validation failed for {}:\n- `## {}` must contain exactly one markdown table",
            relative_path,
            section_title
        );
    }
    let table = tables.into_iter().next().expect("table");
    let headers = expected_headers
        .iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>();
    if table.headers != headers {
        bail!(
            "Draft schema validation failed for {}:\n- `## {}` must use table headers `{}`",
            relative_path,
            section_title,
            expected_headers.join(" | ")
        );
    }
    Ok(table)
}

fn require_section<'a>(relative_path: &str, sections: &'a [Section], title: &str) -> Result<&'a Section> {
    find_section(sections, title).ok_or_else(|| {
        anyhow::anyhow!(
            "Draft schema validation failed for {}:\n- Missing required `## {}` section",
            relative_path,
            title
        )
    })
}

fn find_section<'a>(sections: &'a [Section], title: &str) -> Option<&'a Section> {
    sections.iter().find(|section| section.title == title)
}

fn section_text(sections: &[Section], title: &str) -> String {
    find_section(sections, title)
        .map(|section| section.body.trim().to_string())
        .unwrap_or_default()
}

fn section_body<'a>(sections: &'a [Section], title: &str) -> &'a str {
    find_section(sections, title)
        .map(|section| section.body.as_str())
        .unwrap_or("")
}

fn bullet_lines(body: Option<&str>) -> Vec<String> {
    body.into_iter()
        .flat_map(|value| value.lines())
        .filter_map(|line| {
            let trimmed = line.trim();
            trimmed
                .strip_prefix("- ")
                .or_else(|| trimmed.strip_prefix("* "))
                .map(|value| value.trim().to_string())
        })
        .filter(|value| !value.is_empty())
        .collect()
}

fn parse_named_bullets(body: &str) -> Vec<(String, String)> {
    let bullet_re =
        Regex::new(r#"^\s*[-*]\s*(?:\*\*)?([^:*`]+?)(?:\([^)]*\))?(?:\*\*)?(?:\s*:\s*(.+))?\s*$"#)
            .expect("valid named bullet regex");
    let mut items = Vec::new();
    let mut current: Option<(String, String)> = None;
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(captures) = bullet_re.captures(trimmed) {
            if let Some(existing) = current.take() {
                items.push(existing);
            }
            let name = captures
                .get(1)
                .map(|value| strip_code_ticks(value.as_str()))
                .unwrap_or_default();
            let detail = captures
                .get(2)
                .map(|value| value.as_str().trim().to_string())
                .unwrap_or_default();
            current = Some((name, detail));
            continue;
        }
        if let Some((_, detail)) = current.as_mut() {
            if !detail.is_empty() {
                detail.push(' ');
            }
            detail.push_str(trimmed);
        }
    }
    if let Some(existing) = current {
        items.push(existing);
    }
    items
}

/// Parses the numbered steps under a `**Flow:**` label.
///
/// Collects lines that begin with a decimal number followed by `. ` (e.g. `1. `, `12. `).
/// Stops at the next bold label, markdown table row, or `### ` subsection header.
fn parse_flow_block(body: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut in_flow = false;
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed == "**Flow:**" {
            in_flow = true;
            continue;
        }
        if in_flow {
            if is_bold_label_line(trimmed) || trimmed.starts_with('|') || trimmed.starts_with("### ") {
                break;
            }
            if trimmed.is_empty() {
                continue;
            }
            if let Some(text) = strip_numbered_item(trimmed) {
                items.push(text);
            }
        }
    }
    items
}

/// Parses bullet items (or a single inline value) under a `**Label:**` heading.
///
/// Handles two forms:
/// - Block: `**Label:**` on its own line, followed by `- item` lines.
/// - Inline: `**Label:** some text on the same line`.
///
/// Stops at the next bold label, table row, or `### ` subsection.
fn parse_labeled_items_block(body: &str, label: &str) -> Vec<String> {
    let block_marker = format!("**{label}:**");
    let inline_prefix = format!("**{label}:** ");
    let mut items = Vec::new();
    let mut in_section = false;
    for line in body.lines() {
        let trimmed = line.trim();
        // Inline form: `**Label:** text`
        if let Some(rest) = trimmed.strip_prefix(&inline_prefix) {
            let rest = rest.trim();
            if !rest.is_empty() && rest != "—" {
                items.push(rest.to_string());
            }
            in_section = true;
            continue;
        }
        // Block form: `**Label:**` alone
        if trimmed == block_marker {
            in_section = true;
            continue;
        }
        if in_section {
            if is_bold_label_line(trimmed) || trimmed.starts_with('|') || trimmed.starts_with("### ") {
                break;
            }
            if trimmed.is_empty() {
                continue;
            }
            if let Some(item) = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* ")) {
                let item = item.trim().to_string();
                if item != "—" {
                    items.push(item);
                }
            }
        }
    }
    items
}

/// Returns true when `line` looks like a bold use-case label, e.g. `**Flow:**`.
fn is_bold_label_line(line: &str) -> bool {
    line.starts_with("**") && line.contains(":**")
}

/// Strips a leading ordinal prefix (`1. `, `12. `, …) and returns the remainder.
/// Returns `None` if the line does not start with such a prefix.
fn strip_numbered_item(line: &str) -> Option<String> {
    let dot = line.find(". ")?;
    if dot == 0 {
        return None;
    }
    if line[..dot].chars().all(|c| c.is_ascii_digit()) {
        Some(line[dot + 2..].trim().to_string())
    } else {
        None
    }
}

fn parse_sections(content: &str) -> Vec<Section> {
    let mut sections = Vec::new();
    let mut current_title: Option<String> = None;
    let mut current_body = String::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(title) = trimmed.strip_prefix("## ") {
            if let Some(existing) = current_title.take() {
                sections.push(Section {
                    title: existing,
                    body: current_body.trim().to_string(),
                });
                current_body.clear();
            }
            current_title = Some(title.trim().to_string());
            continue;
        }
        if current_title.is_some() {
            current_body.push_str(line);
            current_body.push('\n');
        }
    }
    if let Some(existing) = current_title {
        sections.push(Section {
            title: existing,
            body: current_body.trim().to_string(),
        });
    }
    sections
}

fn parse_subsections(body: &str) -> Vec<Subsection> {
    let mut subsections = Vec::new();
    let mut current_title: Option<String> = None;
    let mut current_body = String::new();
    for line in body.lines() {
        let trimmed = line.trim();
        if let Some(title) = trimmed.strip_prefix("### ") {
            if let Some(existing) = current_title.take() {
                subsections.push(Subsection {
                    title: existing,
                    body: current_body.trim().to_string(),
                });
                current_body.clear();
            }
            current_title = Some(strip_code_ticks(title));
            continue;
        }
        if current_title.is_some() {
            current_body.push_str(line);
            current_body.push('\n');
        }
    }
    if let Some(existing) = current_title {
        subsections.push(Subsection {
            title: existing,
            body: current_body.trim().to_string(),
        });
    }
    subsections
}

fn parse_tables(body: &str) -> Vec<MarkdownTable> {
    let mut tables = Vec::new();
    let lines = body.lines().collect::<Vec<_>>();
    let mut idx = 0usize;
    while idx < lines.len() {
        if !lines[idx].trim().starts_with('|') {
            idx += 1;
            continue;
        }
        let start = idx;
        while idx < lines.len() && lines[idx].trim().starts_with('|') {
            idx += 1;
        }
        let block = &lines[start..idx];
        if block.len() < 2 || !is_table_separator(block[1]) {
            continue;
        }
        let headers = split_table_row(block[0]);
        let rows = block[2..]
            .iter()
            .filter(|line| !line.trim().is_empty())
            .map(|line| split_table_row(line))
            .collect();
        tables.push(MarkdownTable { headers, rows });
    }
    tables
}

fn is_table_separator(line: &str) -> bool {
    split_table_row(line)
        .into_iter()
        .all(|cell| !cell.is_empty() && cell.chars().all(|ch| matches!(ch, '-' | ':' | ' ')))
}

fn split_table_row(line: &str) -> Vec<String> {
    line.trim()
        .trim_matches('|')
        .split('|')
        .map(|value| value.trim().to_string())
        .collect()
}

fn extract_title(content: &str) -> Option<String> {
    content.lines().find_map(|line| {
        line.trim()
            .strip_prefix("# ")
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

fn first_component(path: &Path) -> Option<&str> {
    match path.components().next()? {
        Component::Normal(value) => value.to_str(),
        _ => None,
    }
}

fn strip_code_ticks(value: &str) -> String {
    value.trim().trim_matches('`').to_string()
}

fn matches_ignore_ascii_case(value: &str, options: &[&str]) -> bool {
    options
        .iter()
        .any(|option| value.eq_ignore_ascii_case(option))
}
