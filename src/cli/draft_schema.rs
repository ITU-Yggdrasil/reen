use anyhow::{Result, bail};
use regex::Regex;
use serde::Serialize;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DraftKind {
    Root,
    Context,
    Projection,
    Data,
    Api,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct DraftDocument {
    pub(crate) kind: DraftKind,
    pub(crate) title: String,
    pub(crate) relative_path: String,
    pub(crate) summary: DraftSummary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum DraftSummary {
    Context(ContextDraftSummary),
    Projection(ProjectionDraftSummary),
    Data(DataDraftSummary),
    Api(ApiDraftSummary),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ContextDraftSummary {
    pub(crate) purpose: String,
    pub(crate) role_players: Vec<RolePlayerRow>,
    pub(crate) role_methods: Vec<RoleMethodGroup>,
    pub(crate) props: Vec<PropRow>,
    pub(crate) functionalities: Vec<ContextFunctionalitySummary>,
    pub(crate) notes: Option<String>,
}

/// Summary for projection drafts. Identical structure to `ContextDraftSummary`
/// but without `message_receiver`: projections are always immutable by kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ProjectionDraftSummary {
    pub(crate) purpose: String,
    pub(crate) role_players: Vec<RolePlayerRow>,
    pub(crate) role_methods: Vec<RoleMethodGroup>,
    pub(crate) props: Vec<PropRow>,
    pub(crate) functionalities: Vec<ContextFunctionalitySummary>,
    pub(crate) notes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct RolePlayerRow {
    pub(crate) role_player: String,
    pub(crate) why_involved: String,
    pub(crate) expected_behaviour: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct PropRow {
    pub(crate) prop: String,
    pub(crate) meaning: String,
    pub(crate) notes: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct RoleMethodGroup {
    pub(crate) role: String,
    pub(crate) methods: Vec<NamedBullet>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct NamedBullet {
    pub(crate) name: String,
    pub(crate) detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ContextFunctionalitySummary {
    pub(crate) name: String,
    pub(crate) interactions: Vec<InteractionRow>,
    pub(crate) rules: Vec<String>,
    pub(crate) examples: Vec<ExampleRow>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct InteractionRow {
    pub(crate) started_by: String,
    pub(crate) uses: String,
    pub(crate) result: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ExampleRow {
    pub(crate) given: String,
    pub(crate) when: String,
    pub(crate) then: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct DataDraftSummary {
    pub(crate) description: String,
    pub(crate) fields: Vec<DataFieldRow>,
    pub(crate) variants: Vec<DataVariantRow>,
    pub(crate) rules: Option<String>,
    pub(crate) construction_rules: Option<String>,
    pub(crate) access_rules: Option<String>,
    pub(crate) functionalities: Option<String>,
    pub(crate) notes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct DataFieldRow {
    pub(crate) field: String,
    pub(crate) meaning: String,
    pub(crate) getter_accessible: bool,
    pub(crate) notes: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct DataVariantRow {
    pub(crate) variant: String,
    pub(crate) meaning: String,
    pub(crate) notes: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ApiDraftSummary {
    pub(crate) description: String,
    pub(crate) authoritative_sources: ApiAuthoritativeSources,
    pub(crate) consumed_surface: BTreeMap<String, Vec<String>>,
    pub(crate) generated_data_specifications: Vec<String>,
    pub(crate) notes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub(crate) struct ApiAuthoritativeSources {
    pub(crate) openapi_url: Option<String>,
    pub(crate) openapi_local: Option<String>,
    pub(crate) documentation_urls: Vec<String>,
    pub(crate) schema_repository_urls: Vec<String>,
}

#[derive(Debug, Clone)]
struct Section {
    title: String,
    body: String,
}

#[derive(Debug, Clone)]
struct Subsection {
    title: String,
    body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MarkdownTable {
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
}

pub(crate) fn infer_draft_kind(draft_file: &Path, drafts_dir: &str) -> DraftKind {
    let drafts_root = PathBuf::from(drafts_dir);
    let relative = draft_file.strip_prefix(&drafts_root).unwrap_or(draft_file);
    match relative
        .components()
        .next()
        .and_then(|component| component.as_os_str().to_str())
    {
        Some("contexts") => DraftKind::Context,
        Some("projections") => DraftKind::Projection,
        Some("data") => DraftKind::Data,
        Some("apis") | Some("external_apis") => DraftKind::Api,
        _ => DraftKind::Root,
    }
}

pub(crate) fn parse_repo_draft(
    draft_file: &Path,
    drafts_dir: &str,
    content: &str,
) -> Result<Option<DraftDocument>> {
    let kind = infer_draft_kind(draft_file, drafts_dir);
    if kind == DraftKind::Root {
        return Ok(None);
    }

    let relative = PathBuf::from(drafts_dir);
    let relative_path = draft_file
        .strip_prefix(&relative)
        .unwrap_or(draft_file)
        .display()
        .to_string();
    let title = extract_title(content).ok_or_else(|| {
        anyhow::anyhow!(
            "Draft schema validation failed for {}:\n- Missing `# <Title>` heading",
            relative_path
        )
    })?;
    let sections = parse_sections(content);

    let summary = match kind {
        DraftKind::Context => {
            DraftSummary::Context(parse_context_summary(&relative_path, &sections)?)
        }
        DraftKind::Projection => {
            DraftSummary::Projection(parse_projection_summary(&relative_path, &sections)?)
        }
        DraftKind::Data => DraftSummary::Data(parse_data_summary(&relative_path, &sections)?),
        DraftKind::Api => DraftSummary::Api(parse_api_summary(&relative_path, &sections)?),
        DraftKind::Root => unreachable!(),
    };

    Ok(Some(DraftDocument {
        kind,
        title,
        relative_path,
        summary,
    }))
}

pub(crate) fn parse_api_draft_content(content: &str) -> Result<ApiDraftSummary> {
    let sections = parse_sections(content);
    parse_api_summary("drafts/apis/<memory>".trim(), &sections)
}

fn parse_context_summary(relative_path: &str, sections: &[Section]) -> Result<ContextDraftSummary> {
    let allowed = [
        "Purpose",
        "Role Players",
        "Role Methods",
        "Props",
        "Functionalities",
        "Notes",
    ];
    validate_section_set(
        relative_path,
        sections,
        &allowed,
        &[
            "Purpose",
            "Role Players",
            "Role Methods",
            "Props",
            "Functionalities",
        ],
    )?;

    let (purpose, role_players, role_methods, props, functionalities, notes) =
        parse_context_like_sections(relative_path, sections)?;

    Ok(ContextDraftSummary {
        purpose,
        role_players,
        role_methods,
        props,
        functionalities,
        notes,
    })
}

fn parse_projection_summary(
    relative_path: &str,
    sections: &[Section],
) -> Result<ProjectionDraftSummary> {
    let allowed = [
        "Purpose",
        "Role Players",
        "Role Methods",
        "Props",
        "Functionalities",
        "Notes",
    ];
    validate_section_set(
        relative_path,
        sections,
        &allowed,
        &[
            "Purpose",
            "Role Players",
            "Role Methods",
            "Props",
            "Functionalities",
        ],
    )?;

    let (purpose, role_players, role_methods, props, functionalities, notes) =
        parse_context_like_sections(relative_path, sections)?;

    Ok(ProjectionDraftSummary {
        purpose,
        role_players,
        role_methods,
        props,
        functionalities,
        notes,
    })
}

/// Shared parsing logic for context-like drafts (both Context and Projection).
fn parse_context_like_sections(
    relative_path: &str,
    sections: &[Section],
) -> Result<(
    String,
    Vec<RolePlayerRow>,
    Vec<RoleMethodGroup>,
    Vec<PropRow>,
    Vec<ContextFunctionalitySummary>,
    Option<String>,
)> {
    let purpose = require_section(relative_path, sections, "Purpose")?
        .body
        .trim()
        .to_string();
    let role_players_table = parse_single_table_section(
        relative_path,
        sections,
        "Role Players",
        &["Role player", "Why involved", "Expected behaviour"],
    )?;
    let props_table = parse_single_table_section(
        relative_path,
        sections,
        "Props",
        &["Prop", "Meaning", "Notes"],
    )?;
    let role_methods = parse_role_methods(
        relative_path,
        require_section(relative_path, sections, "Role Methods")?
            .body
            .as_str(),
    )?;
    let functionalities = parse_context_functionalities(
        relative_path,
        require_section(relative_path, sections, "Functionalities")?
            .body
            .as_str(),
    )?;
    let notes = find_section(sections, "Notes")
        .map(|section| section.body.trim().to_string())
        .filter(|value| !value.is_empty());

    Ok((
        purpose,
        role_players_table
            .rows
            .into_iter()
            .map(|row| RolePlayerRow {
                role_player: row[0].clone(),
                why_involved: row[1].clone(),
                expected_behaviour: row[2].clone(),
            })
            .collect(),
        role_methods,
        props_table
            .rows
            .into_iter()
            .map(|row| PropRow {
                prop: row[0].clone(),
                meaning: row[1].clone(),
                notes: row[2].clone(),
            })
            .collect(),
        functionalities,
        notes,
    ))
}

fn parse_data_summary(relative_path: &str, sections: &[Section]) -> Result<DataDraftSummary> {
    let allowed = [
        "Description",
        "Fields",
        "Variants",
        "Rules",
        "Construction Rules",
        "Access Rules",
        "Functionalities",
        "Notes",
    ];
    validate_section_set(relative_path, sections, &allowed, &["Description"])?;

    let description = require_section(relative_path, sections, "Description")?
        .body
        .trim()
        .to_string();
    let has_fields = find_section(sections, "Fields").is_some();
    let has_variants = find_section(sections, "Variants").is_some();
    if has_fields == has_variants {
        bail!(
            "Draft schema validation failed for {}:\n- Data drafts must contain exactly one of `## Fields` or `## Variants`",
            relative_path
        );
    }

    let fields = if let Some(section) = find_section(sections, "Fields") {
        parse_fields_table(relative_path, section.body.as_str())?
    } else {
        Vec::new()
    };
    let variants = if let Some(section) = find_section(sections, "Variants") {
        parse_variants_table(relative_path, section.body.as_str())?
    } else {
        Vec::new()
    };

    Ok(DataDraftSummary {
        description,
        fields,
        variants,
        rules: optional_section_text(sections, "Rules"),
        construction_rules: optional_section_text(sections, "Construction Rules"),
        access_rules: optional_section_text(sections, "Access Rules"),
        functionalities: optional_section_text(sections, "Functionalities"),
        notes: optional_section_text(sections, "Notes"),
    })
}

fn parse_api_summary(relative_path: &str, sections: &[Section]) -> Result<ApiDraftSummary> {
    let allowed = [
        "Description",
        "Authoritative Sources",
        "Consumed Surface",
        "Generated Data Specifications",
        "Notes",
    ];
    validate_section_set(
        relative_path,
        sections,
        &allowed,
        &["Description", "Authoritative Sources"],
    )?;

    let description = require_section(relative_path, sections, "Description")?
        .body
        .trim()
        .to_string();
    let authoritative_sources = parse_authoritative_sources(
        relative_path,
        require_section(relative_path, sections, "Authoritative Sources")?
            .body
            .as_str(),
    )?;
    let consumed_surface = if let Some(section) = find_section(sections, "Consumed Surface") {
        parse_consumed_surface(relative_path, section.body.as_str())?
    } else {
        BTreeMap::new()
    };
    let generated_data_specifications =
        if let Some(section) = find_section(sections, "Generated Data Specifications") {
            parse_plain_bullet_list(section.body.as_str())
        } else {
            Vec::new()
        };
    let notes = optional_section_text(sections, "Notes");

    if authoritative_sources.openapi_url.is_none() && authoritative_sources.openapi_local.is_none()
    {
        bail!(
            "Draft schema validation failed for {}:\n- `## Authoritative Sources` must include `OpenAPI URL` or `OpenAPI Local`",
            relative_path
        );
    }

    Ok(ApiDraftSummary {
        description,
        authoritative_sources,
        consumed_surface,
        generated_data_specifications,
        notes,
    })
}

fn validate_section_set(
    relative_path: &str,
    sections: &[Section],
    allowed: &[&str],
    required: &[&str],
) -> Result<()> {
    let allowed_set = allowed.iter().copied().collect::<HashSet<_>>();
    let mut seen = HashSet::new();
    let mut errors = Vec::new();

    for section in sections {
        if !allowed_set.contains(section.title.as_str()) {
            errors.push(format!("Unsupported `## {}` section", section.title));
        }
        if !seen.insert(section.title.as_str()) {
            errors.push(format!("Duplicate `## {}` section", section.title));
        }
    }
    for title in required {
        if find_section(sections, title).is_none() {
            errors.push(format!("Missing required `## {}` section", title));
        }
    }

    if errors.is_empty() {
        return Ok(());
    }

    bail!(
        "Draft schema validation failed for {}:\n- {}",
        relative_path,
        errors.join("\n- ")
    );
}

fn require_section<'a>(
    relative_path: &str,
    sections: &'a [Section],
    title: &str,
) -> Result<&'a Section> {
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

fn optional_section_text(sections: &[Section], title: &str) -> Option<String> {
    find_section(sections, title)
        .map(|section| section.body.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn is_markdown_thematic_break(line: &str) -> bool {
    let compact = line
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();
    if compact.len() < 3 {
        return false;
    }
    let mut chars = compact.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    matches!(first, '-' | '*' | '_') && chars.all(|ch| ch == first)
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
    let tables = parse_tables(section.body.as_str());
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

fn parse_role_methods(relative_path: &str, body: &str) -> Result<Vec<RoleMethodGroup>> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    let subsections = parse_subsections(body);
    if subsections.is_empty() {
        bail!(
            "Draft schema validation failed for {}:\n- `## Role Methods` must use `### <Role Name>` subsections when it is not empty",
            relative_path
        );
    }

    let mut groups = Vec::new();
    for subsection in subsections {
        groups.push(RoleMethodGroup {
            role: subsection.title,
            methods: parse_named_bullets(subsection.body.as_str()),
        });
    }
    Ok(groups)
}

fn parse_context_functionalities(
    relative_path: &str,
    body: &str,
) -> Result<Vec<ContextFunctionalitySummary>> {
    let subsections = parse_subsections(body);
    if subsections.is_empty() {
        bail!(
            "Draft schema validation failed for {}:\n- `## Functionalities` must contain at least one `###` subsection",
            relative_path
        );
    }

    let mut summaries = Vec::new();
    for subsection in subsections {
        let tables = parse_tables(subsection.body.as_str());
        let interaction_tables = tables
            .iter()
            .filter(|table| table.headers == ["Started by", "Uses", "Result"])
            .collect::<Vec<_>>();
        let example_tables = tables
            .iter()
            .filter(|table| table.headers == ["Given", "When", "Then"])
            .collect::<Vec<_>>();
        if interaction_tables.len() != 1 {
            bail!(
                "Draft schema validation failed for {}:\n- Functionality `### {}` must contain exactly one `Started by | Uses | Result` table",
                relative_path,
                subsection.title
            );
        }
        if example_tables.len() != 1 {
            bail!(
                "Draft schema validation failed for {}:\n- Functionality `### {}` must contain exactly one `Given | When | Then` table",
                relative_path,
                subsection.title
            );
        }

        let rules_blocks = parse_rules_blocks(subsection.body.as_str());
        if rules_blocks.len() != 1 {
            bail!(
                "Draft schema validation failed for {}:\n- Functionality `### {}` must contain exactly one `Rules:` bullet list",
                relative_path,
                subsection.title
            );
        }

        summaries.push(ContextFunctionalitySummary {
            name: subsection.title,
            interactions: interaction_tables[0]
                .rows
                .iter()
                .map(|row| InteractionRow {
                    started_by: row[0].clone(),
                    uses: row[1].clone(),
                    result: row[2].clone(),
                })
                .collect(),
            rules: rules_blocks.into_iter().next().unwrap_or_default(),
            examples: example_tables[0]
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

    Ok(summaries)
}

fn parse_fields_table(relative_path: &str, body: &str) -> Result<Vec<DataFieldRow>> {
    let tables = parse_tables(body);
    if tables.len() != 1 {
        bail!(
            "Draft schema validation failed for {}:\n- `## Fields` must contain exactly one markdown table",
            relative_path
        );
    }
    let table = tables.into_iter().next().expect("table");
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
    for row in table.rows {
        if row.len() != table.headers.len() {
            bail!(
                "Draft schema validation failed for {}:\n- Every `## Fields` row must have {} column(s)",
                relative_path,
                table.headers.len()
            );
        }
        let (getter_accessible, notes) = if has_accessible {
            let value = row[2].trim();
            let getter_accessible = if value.is_empty() {
                // Empty Accessible cell in the 4-column format means opt-out (no getter).
                false
            } else if matches_ignore_ascii_case(value, &["x", "yes", "true"]) {
                true
            } else if matches_ignore_ascii_case(value, &["no", "false"]) {
                false
            } else {
                bail!(
                    "Draft schema validation failed for {}:\n- `Accessible` must be blank, `no`/`false` to suppress, or one of `X`, `yes`, `true` to expose (case-insensitive), got `{}`",
                    relative_path,
                    value
                );
            };
            (getter_accessible, row[3].clone())
        } else {
            // 3-column format: all fields are getter-accessible by default.
            (true, row[2].clone())
        };

        rows.push(DataFieldRow {
            field: row[0].clone(),
            meaning: row[1].clone(),
            getter_accessible,
            notes,
        });
    }
    Ok(rows)
}

fn parse_variants_table(relative_path: &str, body: &str) -> Result<Vec<DataVariantRow>> {
    let tables = parse_tables(body);
    if tables.len() != 1 {
        bail!(
            "Draft schema validation failed for {}:\n- `## Variants` must contain exactly one markdown table",
            relative_path
        );
    }
    let table = tables.into_iter().next().expect("table");
    if table.headers != ["Variant", "Meaning", "Notes"] {
        bail!(
            "Draft schema validation failed for {}:\n- `## Variants` must use table headers `Variant | Meaning | Notes`",
            relative_path
        );
    }

    Ok(table
        .rows
        .into_iter()
        .map(|row| DataVariantRow {
            variant: row[0].clone(),
            meaning: row[1].clone(),
            notes: row[2].clone(),
        })
        .collect())
}

fn parse_authoritative_sources(relative_path: &str, body: &str) -> Result<ApiAuthoritativeSources> {
    let parsed = parse_labeled_bullets(
        relative_path,
        "Authoritative Sources",
        body,
        &[
            "OpenAPI URL",
            "OpenAPI Local",
            "Documentation URL",
            "Schema Repository URL",
        ],
    )?;

    Ok(ApiAuthoritativeSources {
        openapi_url: parsed
            .get("OpenAPI URL")
            .and_then(|values| values.first())
            .cloned(),
        openapi_local: parsed
            .get("OpenAPI Local")
            .and_then(|values| values.first())
            .cloned(),
        documentation_urls: parsed.get("Documentation URL").cloned().unwrap_or_default(),
        schema_repository_urls: parsed
            .get("Schema Repository URL")
            .cloned()
            .unwrap_or_default(),
    })
}

fn parse_consumed_surface(
    relative_path: &str,
    body: &str,
) -> Result<BTreeMap<String, Vec<String>>> {
    let parsed = parse_labeled_bullets(
        relative_path,
        "Consumed Surface",
        body,
        &[
            "Operations",
            "WebSocket Streams",
            "Schema Types",
            "Message Families",
            "Endpoint Groups",
        ],
    )?;
    Ok(parsed
        .into_iter()
        .map(|(key, values)| {
            (
                key,
                values
                    .into_iter()
                    .flat_map(|value| split_csv_like(value.as_str()))
                    .collect::<Vec<_>>(),
            )
        })
        .collect())
}

fn parse_labeled_bullets(
    relative_path: &str,
    section_title: &str,
    body: &str,
    allowed_labels: &[&str],
) -> Result<BTreeMap<String, Vec<String>>> {
    let label_re = Regex::new(r"^\s*[-*]\s*(?:\*\*)?([^:*]+?)(?:\*\*)?\s*:\s*(\S.+?)\s*$")
        .expect("valid labeled-bullet regex");
    let allowed = allowed_labels.iter().copied().collect::<HashSet<_>>();
    let mut values: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let captures = label_re.captures(trimmed).ok_or_else(|| {
            anyhow::anyhow!(
                "Draft schema validation failed for {}:\n- `## {}` must use labeled bullet lines like `- Label: value`",
                relative_path,
                section_title
            )
        })?;
        let label = captures
            .get(1)
            .map(|m| m.as_str().trim())
            .unwrap_or_default();
        let value = captures
            .get(2)
            .map(|m| m.as_str().trim())
            .unwrap_or_default();
        if !allowed.contains(label) {
            bail!(
                "Draft schema validation failed for {}:\n- `## {}` does not support label `{}`",
                relative_path,
                section_title,
                label
            );
        }
        values
            .entry(label.to_string())
            .or_default()
            .push(value.to_string());
    }

    Ok(values)
}

fn parse_plain_bullet_list(body: &str) -> Vec<String> {
    body.lines()
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

fn parse_named_bullets(body: &str) -> Vec<NamedBullet> {
    let bullet_re =
        Regex::new(r#"^\s*[-*]\s*(?:\*\*)?([^:*`]+?)(?:\([^)]*\))?(?:\*\*)?(?:\s*:\s*(.+))?\s*$"#)
            .expect("valid named-bullet regex");
    let mut items = Vec::new();
    let mut current: Option<NamedBullet> = None;

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
                .map(|m| m.as_str().trim().trim_matches('`').to_string())
                .unwrap_or_default();
            let detail = captures
                .get(2)
                .map(|m| m.as_str().trim().to_string())
                .unwrap_or_default();
            current = Some(NamedBullet { name, detail });
            continue;
        }

        if let Some(existing) = current.as_mut() {
            if !existing.detail.is_empty() {
                existing.detail.push(' ');
            }
            existing.detail.push_str(trimmed);
        }
    }

    if let Some(existing) = current {
        items.push(existing);
    }
    items.retain(|item| !item.name.is_empty());
    items
}

fn parse_rules_blocks(body: &str) -> Vec<Vec<String>> {
    let lines = body.lines().collect::<Vec<_>>();
    let mut blocks = Vec::new();
    let mut idx = 0usize;
    while idx < lines.len() {
        if lines[idx].trim() != "Rules:" {
            idx += 1;
            continue;
        }
        idx += 1;
        let mut rules = Vec::new();
        while idx < lines.len() {
            let trimmed = lines[idx].trim();
            if trimmed.is_empty() {
                idx += 1;
                continue;
            }
            if trimmed.starts_with('|') || trimmed.starts_with("### ") || trimmed == "Rules:" {
                break;
            }
            if let Some(rule) = trimmed
                .strip_prefix("- ")
                .or_else(|| trimmed.strip_prefix("* "))
            {
                rules.push(rule.trim().to_string());
                idx += 1;
                continue;
            }
            break;
        }
        if !rules.is_empty() {
            blocks.push(rules);
        }
    }
    blocks
}

fn parse_subsections(body: &str) -> Vec<Subsection> {
    let mut subsections = Vec::new();
    let mut current_title: Option<String> = None;
    let mut current_body = String::new();

    for line in body.lines() {
        if let Some(title) = line.trim().strip_prefix("### ") {
            if let Some(existing) = current_title.take() {
                subsections.push(Subsection {
                    title: existing,
                    body: current_body.trim().to_string(),
                });
                current_body.clear();
            }
            current_title = Some(title.trim().trim_matches('`').to_string());
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
        if current_title.is_some() && is_markdown_thematic_break(trimmed) {
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
            .collect::<Vec<_>>();
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
        .map(|cell| cell.trim().to_string())
        .collect()
}

fn split_csv_like(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

fn extract_title(content: &str) -> Option<String> {
    content.lines().find_map(|line| {
        line.trim()
            .strip_prefix("# ")
            .map(|title| title.trim().to_string())
            .filter(|title| !title.is_empty())
    })
}

/// First Markdown `# ` heading line (same rule as [`parse_repo_draft`]).
pub(crate) fn draft_title_from_markdown(content: &str) -> Option<String> {
    extract_title(content)
}

fn matches_ignore_ascii_case(value: &str, options: &[&str]) -> bool {
    options
        .iter()
        .any(|option| value.eq_ignore_ascii_case(option))
}

#[cfg(test)]
mod tests {
    use super::{
        ApiDraftSummary, DraftKind, DraftSummary, infer_draft_kind, parse_api_draft_content,
        parse_repo_draft,
    };
    use std::fs;
    use std::path::Path;

    fn collect_markdown_files(root: &Path, files: &mut Vec<std::path::PathBuf>) {
        let Ok(entries) = fs::read_dir(root) else {
            return;
        };
        for entry in entries {
            let entry = entry.expect("dir entry");
            let path = entry.path();
            if path.is_dir() {
                collect_markdown_files(&path, files);
                continue;
            }
            if path.extension().and_then(|value| value.to_str()) == Some("md") {
                files.push(path);
            }
        }
    }

    #[test]
    fn infers_kind_from_path() {
        assert_eq!(
            infer_draft_kind(Path::new("drafts/contexts/game_loop.md"), "drafts"),
            DraftKind::Context
        );
        assert_eq!(
            infer_draft_kind(Path::new("drafts/data/board.md"), "drafts"),
            DraftKind::Data
        );
        assert_eq!(
            infer_draft_kind(Path::new("drafts/apis/aisstream.md"), "drafts"),
            DraftKind::Api
        );
        assert_eq!(
            infer_draft_kind(Path::new("drafts/app.md"), "drafts"),
            DraftKind::Root
        );
    }

    #[test]
    fn parses_valid_context_draft() {
        let content = r#"# BufferContext

## Purpose

Stores recent events.

## Role Players

| Role player | Why involved | Expected behaviour |
|---|---|---|
| clock | Defines time | Returns the current UTC time |

## Role Methods

### clock

- **now** Returns the current UTC time.

## Props

| Prop | Meaning | Notes |
|---|---|---|
| window | Retention window | Minutes |

## Functionalities

### receive_event

| Started by | Uses | Result |
|---|---|---|
| receiver | event | event is stored |

Rules:
- The event is appended.

| Given | When | Then |
|---|---|---|
| a valid event | receive_event runs | the event is stored |
"#;

        let parsed = parse_repo_draft(Path::new("drafts/contexts/buffer.md"), "drafts", content)
            .expect("parse")
            .expect("summary");
        match parsed.summary {
            DraftSummary::Context(summary) => {
                assert_eq!(summary.role_players.len(), 1);
                assert_eq!(summary.functionalities.len(), 1);
                assert_eq!(
                    summary.functionalities[0].rules,
                    vec!["The event is appended."]
                );
            }
            other => panic!("unexpected summary: {other:?}"),
        }
    }

    #[test]
    fn rejects_context_with_unsupported_message_receiver_section() {
        // Message Receiver is no longer a valid section for context drafts.
        let content = r#"# BufferContext

## Purpose

Stores recent events.

## Message Receiver

yes

## Role Players

| Role player | Why involved | Expected behaviour |
|---|---|---|
| clock | Defines time | Returns the current UTC time |

## Role Methods

### clock

- **now** Returns the current UTC time.

## Props

| Prop | Meaning | Notes |
|---|---|---|
| window | Retention window | Minutes |

## Functionalities

### receive_event

| Started by | Uses | Result |
|---|---|---|
| receiver | event | event is stored |

Rules:
- The event is appended.

| Given | When | Then |
|---|---|---|
| a valid event | receive_event runs | the event is stored |
"#;

        let error = parse_repo_draft(Path::new("drafts/contexts/buffer.md"), "drafts", content)
            .expect_err("should fail");
        assert!(
            error.to_string().contains("Unsupported"),
            "expected unsupported-section error, got: {}",
            error
        );
    }

    #[test]
    fn parses_projection_draft() {
        let content = r#"# CommandInputProjection

## Purpose

Collects and reads key presses.

## Role Players

| Role player | Why involved | Expected behaviour |
|---|---|---|
| stdin_source | Supplies keyboard input | Provides non-blocking reads |

## Role Methods

### stdin_source

- **read_available** Returns all currently available keystrokes.

## Props

| Prop | Meaning | Notes |
|---|---|---|
| buffer | FIFO queue | Shared for the whole session |

## Functionalities

### capture

| Started by | Uses | Result |
|---|---|---|
| game loop | stdin_source, buffer | keys are buffered |

Rules:
- Reads currently available keys.

| Given | When | Then |
|---|---|---|
| keys are available | capture runs | keys are buffered |
"#;

        let parsed = parse_repo_draft(
            Path::new("drafts/projections/command_input.md"),
            "drafts",
            content,
        )
        .expect("parse")
        .expect("summary");
        assert_eq!(parsed.kind, DraftKind::Projection);
        match parsed.summary {
            DraftSummary::Projection(summary) => {
                assert_eq!(summary.purpose, "Collects and reads key presses.");
                assert_eq!(summary.role_players.len(), 1);
                assert_eq!(summary.functionalities.len(), 1);
            }
            other => panic!("unexpected summary: {other:?}"),
        }
    }

    #[test]
    fn rejects_context_without_required_functionality_parts() {
        let content = r#"# BufferContext

## Purpose

Stores recent events.

## Role Players

| Role player | Why involved | Expected behaviour |
|---|---|---|
| clock | Defines time | Returns the current UTC time |

## Role Methods

### clock

- **now** Returns the current UTC time.

## Props

| Prop | Meaning | Notes |
|---|---|---|
| window | Retention window | Minutes |

## Functionalities

### receive_event

| Started by | Uses | Result |
|---|---|---|
| receiver | event | event is stored |
"#;

        let error = parse_repo_draft(Path::new("drafts/contexts/buffer.md"), "drafts", content)
            .expect_err("should fail");
        assert!(error.to_string().contains("must contain exactly one"));
    }

    #[test]
    fn parses_fields_with_accessible_column() {
        let content = r#"# PositionEvent

## Description

Shared event record.

## Fields

| Field | Meaning | Accessible | Notes |
|---|---|---|---|
| latitude | Event latitude | yes | Decimal degrees |
| longitude | Event longitude |  | Decimal degrees |
"#;

        let parsed = parse_repo_draft(
            Path::new("drafts/data/position_event.md"),
            "drafts",
            content,
        )
        .expect("parse")
        .expect("summary");
        match parsed.summary {
            DraftSummary::Data(summary) => {
                assert_eq!(summary.fields.len(), 2);
                assert!(summary.fields[0].getter_accessible);
                assert!(!summary.fields[1].getter_accessible);
            }
            other => panic!("unexpected summary: {other:?}"),
        }
    }

    #[test]
    fn rejects_invalid_accessible_value() {
        let content = r#"# PositionEvent

## Description

Shared event record.

## Fields

| Field | Meaning | Accessible | Notes |
|---|---|---|---|
| latitude | Event latitude | maybe | Decimal degrees |
"#;

        let error = parse_repo_draft(
            Path::new("drafts/data/position_event.md"),
            "drafts",
            content,
        )
        .expect_err("should fail");
        assert!(error.to_string().contains("Accessible"));
    }

    #[test]
    fn rejects_fields_and_variants_together() {
        let content = r#"# Direction

## Description

Direction enum.

## Fields

| Field | Meaning | Notes |
|---|---|---|
| value | Stored value | Text |

## Variants

| Variant | Meaning | Notes |
|---|---|---|
| Up | Upward movement | |
"#;

        let error = parse_repo_draft(Path::new("drafts/data/direction.md"), "drafts", content)
            .expect_err("should fail");
        assert!(error.to_string().contains("exactly one"));
    }

    #[test]
    fn parses_variant_table_for_enum_data() {
        let content = r#"# Direction

## Description

Direction enum.

## Variants

| Variant | Meaning | Notes |
|---|---|---|
| Up | Upward movement | |
| Down | Downward movement | |
"#;

        let parsed = parse_repo_draft(Path::new("drafts/data/direction.md"), "drafts", content)
            .expect("parse")
            .expect("summary");
        match parsed.summary {
            DraftSummary::Data(summary) => {
                assert_eq!(summary.variants.len(), 2);
                assert!(summary.fields.is_empty());
            }
            other => panic!("unexpected summary: {other:?}"),
        }
    }

    #[test]
    fn parses_api_labeled_bullets() {
        let content = r#"# AISStream

## Description

Realtime vessel tracking.

## Authoritative Sources

- OpenAPI URL: https://example.com/openapi.yaml
- Documentation URL: https://docs.example.com/aisstream
- Schema Repository URL: https://github.com/example/aisstream-schema

## Consumed Surface

- Operations: GET /v1/charges, GET /v1/customers
- Message Families: PositionReport, StandardClassBPositionReport

## Generated Data Specifications

- AisMessageTypes
- AisStreamMessage
"#;

        let parsed = parse_api_draft_content(content).expect("parse");
        let ApiDraftSummary {
            authoritative_sources,
            consumed_surface,
            generated_data_specifications,
            ..
        } = parsed;
        assert_eq!(
            authoritative_sources.openapi_url.as_deref(),
            Some("https://example.com/openapi.yaml")
        );
        assert_eq!(
            authoritative_sources.documentation_urls,
            vec!["https://docs.example.com/aisstream".to_string()]
        );
        assert_eq!(
            consumed_surface.get("Operations"),
            Some(&vec![
                "GET /v1/charges".to_string(),
                "GET /v1/customers".to_string()
            ])
        );
        assert_eq!(
            generated_data_specifications,
            vec![
                "AisMessageTypes".to_string(),
                "AisStreamMessage".to_string()
            ]
        );
    }

    #[test]
    fn rejects_legacy_api_sections_in_strict_mode() {
        let content = r#"# AISStream

## Description

Realtime vessel tracking.

## API Specification

- https://example.com/openapi.yaml
"#;

        let error = parse_repo_draft(Path::new("drafts/apis/aisstream.md"), "drafts", content)
            .expect_err("should fail");
        assert!(error.to_string().contains("Unsupported"));
    }

    #[test]
    fn parses_repo_fixture_drafts_in_strict_mode() {
        let fixture_roots = [
            "tests/snake/drafts",
            "tests/money transfer/drafts",
            "tests/Connor/drafts",
            "tests/world data/drafts",
        ];

        for drafts_dir in fixture_roots {
            let drafts_root = Path::new(drafts_dir);
            let mut markdown_files = Vec::new();
            collect_markdown_files(drafts_root, &mut markdown_files);
            markdown_files.sort();

            for draft_file in markdown_files {
                if draft_file.ends_with("test_app.html") {
                    continue;
                }
                let content = match fs::read_to_string(&draft_file) {
                    Ok(content) => content,
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(err) => panic!("read draft: {err}"),
                };
                let parsed =
                    parse_repo_draft(&draft_file, drafts_dir, &content).unwrap_or_else(|err| {
                        panic!("failed to parse {}: {err}", draft_file.display())
                    });

                if draft_file.file_name().and_then(|value| value.to_str()) == Some("app.md") {
                    assert!(parsed.is_none(), "root app draft should not be parsed");
                } else {
                    assert!(
                        parsed.is_some(),
                        "expected parsed summary for {}",
                        draft_file.display()
                    );
                }
            }
        }
    }
}
