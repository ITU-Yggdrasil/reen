use anyhow::{Result, bail};
use std::path::{Component, Path};

// ── Artifact kinds ────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ArtifactKind {
    Data,
    Projection,
    Context,
    Operation,
    Component,
    Brand,
    RoutingTable,
    App,
}

impl ArtifactKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ArtifactKind::Data => "data",
            ArtifactKind::Projection => "projection",
            ArtifactKind::Context => "context",
            ArtifactKind::Operation => "operation",
            ArtifactKind::Component => "component",
            ArtifactKind::Brand => "brand",
            ArtifactKind::RoutingTable => "routing_table",
            ArtifactKind::App => "app",
        }
    }

    pub fn from_draft_path(relative_path: &Path) -> Option<Self> {
        // Special case: drafts/app.md is at the root of drafts, not in a subdirectory
        if relative_path.to_string_lossy() == "app.md" {
            return Some(Self::App);
        }
        match first_component(relative_path)? {
            "data" => Some(Self::Data),
            "projections" => Some(Self::Projection),
            "contexts" => Some(Self::Context),
            "components" => Some(Self::Component),
            "brands" => Some(Self::Brand),
            "routes" => Some(Self::RoutingTable),
            _ => None,
        }
    }

    pub fn from_prepared_path(relative_path: &Path) -> Option<Self> {
        // Special case: app.yml is at the root of prepare, not in a subdirectory
        if relative_path.to_string_lossy() == "app.yml" {
            return Some(Self::App);
        }
        match first_component(relative_path)? {
            "data" => Some(Self::Data),
            "projections" => Some(Self::Projection),
            "contexts" => Some(Self::Context),
            "operations" => Some(Self::Operation),
            "components" => Some(Self::Component),
            "brands" => Some(Self::Brand),
            "routes" => Some(Self::RoutingTable),
            _ => None,
        }
    }
}

// ── Draft info ────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftInfo {
    pub kind: ArtifactKind,
    pub relative_path: String,
    pub title: String,
}

// ── Parsed draft types (BA-authorable, DCI-native) ────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScenarioDraft {
    pub name: String,
    pub given: String,
    pub when: String,
    pub then: String,
}

// ── Context draft ─────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RolePlayerDraft {
    pub name: String,
    pub data_type: String,
    pub responsibility: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleMethodDraft {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleMethodGroupDraft {
    pub role: String,
    pub methods: Vec<RoleMethodDraft>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropDraft {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionalityDraft {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextDraft {
    pub info: DraftInfo,
    pub purpose: String,
    pub role_players: Vec<RolePlayerDraft>,
    pub props: Vec<PropDraft>,
    pub role_methods: Vec<RoleMethodGroupDraft>,
    pub functionalities: Vec<FunctionalityDraft>,
    pub scenarios: Vec<ScenarioDraft>,
}

// ── Data draft ────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributeDraft {
    pub name: String,
    pub description: String,
}

/// One variant of a sum-typed Data draft. Authored under `## Attributes` as a
/// `### <name> attributes` H3 sub-section. Top-level attributes (those above
/// the first H3) remain on `DataDraft.attributes` and are shared across
/// variants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataVariantDraft {
    /// Canonical variant name (snake_case). Derived from the H3 heading by
    /// lowercasing, stripping a trailing ` attributes` suffix (BA convenience),
    /// and replacing spaces and dashes with underscores.
    pub name: String,
    pub attributes: Vec<AttributeDraft>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationDraft {
    pub name: String,
    pub description: String,
    /// Kind marker — e.g. `Some("with")` for non-destructive mutation.
    /// Parsed from `(kind: <value>)` parenthetical in the operation title/bullet.
    pub kind: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataDraft {
    pub info: DraftInfo,
    pub purpose: String,
    pub attributes: Vec<AttributeDraft>,
    pub invariants: Vec<String>,
    pub operations: Vec<OperationDraft>,
    pub scenarios: Vec<ScenarioDraft>,
    /// Variants modelled as `### <name> attributes` H3 sub-sections under
    /// `## Attributes`. Empty for non-variant data types.
    pub variants: Vec<DataVariantDraft>,
}

// ── Projection draft ──────────────────────────────────────────────
//
// A Projection draft is structurally identical to a Context draft: same six
// sections (Purpose, Role Players, Props, Role Methods, Functionalities,
// Scenarios). The only difference is immutability, which is enforced at the
// schema, codegen, and verify layers — not at the draft level.
//
// The legacy `## Source` / `## Queries` shape is rejected at parse time with a
// migration-pointing error. Callers must rewrite old drafts to the new shape.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionDraft {
    pub info: DraftInfo,
    pub purpose: String,
    pub role_players: Vec<RolePlayerDraft>,
    pub props: Vec<PropDraft>,
    pub role_methods: Vec<RoleMethodGroupDraft>,
    pub functionalities: Vec<FunctionalityDraft>,
    pub scenarios: Vec<ScenarioDraft>,
}

// ── Component draft ──────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotDraft {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventDraft {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantDraft {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildBindingDraft {
    pub child: String,
    pub bindings: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventBindingDraft {
    pub child: String,
    pub event: String,
    pub context: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompositionDraft {
    pub child_bindings: Vec<ChildBindingDraft>,
    pub event_bindings: Vec<EventBindingDraft>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentDraft {
    pub info: DraftInfo,
    pub purpose: String,
    pub props: Vec<PropDraft>,
    pub slots: Vec<SlotDraft>,
    pub events: Vec<EventDraft>,
    pub variants: Vec<VariantDraft>,
    pub composition: Option<CompositionDraft>,
    pub invariants: Vec<String>,
    pub scenarios: Vec<ScenarioDraft>,
}

// ── Brand draft ──────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrandTokenDraft {
    pub name: String,
    pub role: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrandTokenGroupDraft {
    pub group: String,
    pub tokens: Vec<BrandTokenDraft>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrandProseSectionDraft {
    pub name: String,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrandDraft {
    pub info: DraftInfo,
    pub purpose: String,
    pub voice: Option<BrandProseSectionDraft>,
    pub token_groups: Vec<BrandTokenGroupDraft>,
    pub prose_sections: Vec<BrandProseSectionDraft>,
}

// ── Routing table draft ──────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteParamDraft {
    pub name: String,
    pub data_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteDraft {
    pub path: String,
    pub component: String,
    pub params: Vec<RouteParamDraft>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutingTableDraft {
    pub info: DraftInfo,
    pub purpose: String,
    pub routes: Vec<RouteDraft>,
}

// ── App draft ────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppDraft {
    pub info: DraftInfo,
    pub application_kind: String,
    pub entry_context: Option<String>,
    pub entry_functionality: Option<String>,
    pub dependencies: Vec<String>,
    pub notes: String,
}

// ── Boundary draft (API) ─────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundaryDraft {
    pub api_name: String,
    pub purpose: String,
    pub source: BoundarySource,
    /// Operation names to whitelist; empty means "all operations".
    pub operations_whitelist: Vec<String>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundarySource {
    pub openapi_local: Option<String>,
    pub openapi_url: Option<String>,
    pub documentation_url: Option<String>,
}

// ── Top-level enum ────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DraftDocument {
    Data(DataDraft),
    Projection(ProjectionDraft),
    Context(ContextDraft),
    Component(ComponentDraft),
    Brand(BrandDraft),
    RoutingTable(RoutingTableDraft),
    App(AppDraft),
}

// ── Entry point ───────────────────────────────────────────────────

pub fn parse_draft_file(path: &Path, drafts_root: &Path, content: &str) -> Result<DraftDocument> {
    let relative_path = path
        .strip_prefix(drafts_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    let kind = ArtifactKind::from_draft_path(Path::new(&relative_path))
        .ok_or_else(|| anyhow::anyhow!("Unsupported draft path: {}", relative_path))?;
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
            parse_projection_draft(info, content).map(DraftDocument::Projection)
        }
        ArtifactKind::Context => parse_context_draft(info, content).map(DraftDocument::Context),
        ArtifactKind::Component => {
            parse_component_draft(info, content).map(DraftDocument::Component)
        }
        ArtifactKind::Brand => parse_brand_draft(info, content).map(DraftDocument::Brand),
        ArtifactKind::RoutingTable => {
            parse_routing_table_draft(info, content).map(DraftDocument::RoutingTable)
        }
        ArtifactKind::App => parse_app_draft(info, content).map(DraftDocument::App),
        ArtifactKind::Operation => {
            bail!("Operation artifacts are YAML, not markdown drafts: {}", info.relative_path)
        }
    }
}

// ── Context parser ────────────────────────────────────────────────

fn parse_context_draft(info: DraftInfo, content: &str) -> Result<ContextDraft> {
    let sections = parse_sections(content);

    let purpose = require_section_text(&info.relative_path, &sections, "Purpose")?;
    let role_players = parse_role_players(&info.relative_path, &sections)?;
    let props = parse_props(&sections);
    let role_methods = parse_role_method_groups(&info.relative_path, &sections)?;
    let functionalities = parse_functionality_subsections(&info.relative_path, &sections)?;
    let scenarios = parse_scenarios(&sections);

    Ok(ContextDraft {
        info,
        purpose,
        role_players,
        props,
        role_methods,
        functionalities,
        scenarios,
    })
}

fn parse_role_players(relative_path: &str, sections: &[Section]) -> Result<Vec<RolePlayerDraft>> {
    let section = require_section(relative_path, sections, "Role Players")?;
    let body = section.body.trim();
    if body.is_empty() {
        return Ok(Vec::new());
    }
    // Role players are bullet items: `- <role>: <data type> — <responsibility>`
    let mut players = Vec::new();
    for line in body.lines() {
        let trimmed = line.trim();
        let Some(rest) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        else {
            continue;
        };
        let Some((name, rest)) = rest.split_once(':') else {
            bail!(
                "Draft validation failed for {}:\n- Role player line missing `:`: {}",
                relative_path,
                trimmed
            );
        };
        let name = name.trim().to_string();
        // Split on " — " (em-dash) or " - " (hyphen with spaces) for responsibility
        let (data_type, responsibility) = if let Some(idx) = rest.find(" — ") {
            (rest[..idx].trim().to_string(), rest[idx + 5..].trim().to_string())
        } else if let Some(idx) = rest.find(" - ") {
            // Only split on " - " if it's not at the start
            let before = rest[..idx].trim();
            if before.is_empty() {
                (rest.trim().to_string(), String::new())
            } else {
                (before.to_string(), rest[idx + 3..].trim().to_string())
            }
        } else {
            (rest.trim().to_string(), String::new())
        };
        players.push(RolePlayerDraft {
            name,
            data_type,
            responsibility,
        });
    }
    Ok(players)
}

fn parse_props(sections: &[Section]) -> Vec<PropDraft> {
    let Some(section) = find_section(sections, "Props") else {
        return Vec::new();
    };
    parse_named_bullets(&section.body)
        .into_iter()
        .map(|(name, desc)| PropDraft {
            name,
            description: desc,
        })
        .collect()
}

fn parse_role_method_groups(
    relative_path: &str,
    sections: &[Section],
) -> Result<Vec<RoleMethodGroupDraft>> {
    let section = require_section(relative_path, sections, "Role Methods")?;
    let subsections = parse_subsections(&section.body);
    if subsections.is_empty() {
        bail!(
            "Draft validation failed for {}:\n- `## Role Methods` must use `### <Role Name>` subsections",
            relative_path
        );
    }
    Ok(subsections
        .into_iter()
        .map(|sub| RoleMethodGroupDraft {
            role: sub.title,
            methods: parse_named_bullets(&sub.body)
                .into_iter()
                .map(|(name, desc)| RoleMethodDraft {
                    name,
                    description: desc,
                })
                .collect(),
        })
        .collect())
}

fn parse_functionality_subsections(
    relative_path: &str,
    sections: &[Section],
) -> Result<Vec<FunctionalityDraft>> {
    let section = require_section(relative_path, sections, "Functionalities")?;
    let subsections = parse_subsections(&section.body);
    if subsections.is_empty() {
        bail!(
            "Draft validation failed for {}:\n- `## Functionalities` must contain `### <name>` subsections",
            relative_path
        );
    }
    Ok(subsections
        .into_iter()
        .map(|sub| FunctionalityDraft {
            name: sub.title,
            description: sub.body.trim().to_string(),
        })
        .collect())
}

// ── Data parser ───────────────────────────────────────────────────

fn parse_data_draft(info: DraftInfo, content: &str) -> Result<DataDraft> {
    let sections = parse_sections(content);

    let purpose = require_section_text(&info.relative_path, &sections, "Purpose")?;
    let attributes = parse_attribute_bullets(&sections);
    let variants = parse_data_variants(&sections);
    let invariants = bullet_lines(find_section(&sections, "Invariants").map(|s| s.body.as_str()));
    let operations = parse_operation_subsections(&sections);
    let scenarios = parse_scenarios(&sections);

    Ok(DataDraft {
        info,
        purpose,
        attributes,
        invariants,
        operations,
        scenarios,
        variants,
    })
}

fn parse_attribute_bullets(sections: &[Section]) -> Vec<AttributeDraft> {
    let Some(section) = find_section(sections, "Attributes") else {
        return Vec::new();
    };
    // Top-level attributes are bullets above the first `### …` H3 marker.
    // Anything past the H3 belongs to a variant and is parsed by
    // `parse_data_variants`.
    let top_level = body_before_first_h3(&section.body);
    parse_named_bullets(&top_level)
        .into_iter()
        .map(|(name, desc)| AttributeDraft {
            name,
            description: desc,
        })
        .collect()
}

/// Extract `### …` H3 sub-sections under `## Attributes` as variant groups.
/// Each variant's attributes are the bullets inside its block. The variant
/// name is derived by lowercasing the heading, stripping a trailing
/// ` attributes` suffix (BA-convenience), and converting to snake_case.
fn parse_data_variants(sections: &[Section]) -> Vec<DataVariantDraft> {
    let Some(section) = find_section(sections, "Attributes") else {
        return Vec::new();
    };
    parse_subsections(&section.body)
        .into_iter()
        .map(|sub| DataVariantDraft {
            name: canonicalise_variant_name(&sub.title),
            attributes: parse_named_bullets(&sub.body)
                .into_iter()
                .map(|(name, description)| AttributeDraft { name, description })
                .collect(),
        })
        .collect()
}

fn body_before_first_h3(body: &str) -> String {
    let mut out = String::new();
    for line in body.lines() {
        if line.trim_start().starts_with("### ") {
            break;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn canonicalise_variant_name(title: &str) -> String {
    let trimmed = title.trim().to_lowercase();
    let without_suffix = trimmed
        .strip_suffix(" attributes")
        .unwrap_or(&trimmed);
    let mut out: String = without_suffix
        .chars()
        .map(|c| if c == ' ' || c == '-' { '_' } else { c })
        .collect();
    while out.starts_with('_') {
        out.remove(0);
    }
    while out.ends_with('_') {
        out.pop();
    }
    out
}

fn parse_operation_subsections(sections: &[Section]) -> Vec<OperationDraft> {
    let Some(section) = find_section(sections, "Operations") else {
        return Vec::new();
    };
    let subsections = parse_subsections(&section.body);
    if subsections.is_empty() {
        // Fall back to bullet items
        return parse_named_bullets(&section.body)
            .into_iter()
            .map(|(name, desc)| {
                let (clean_name, kind) = extract_kind_marker(&name);
                OperationDraft {
                    name: clean_name,
                    description: desc,
                    kind,
                }
            })
            .collect();
    }
    subsections
        .into_iter()
        .map(|sub| {
            let (clean_name, kind) = extract_kind_marker(&sub.title);
            OperationDraft {
                name: clean_name,
                description: sub.body.trim().to_string(),
                kind,
            }
        })
        .collect()
}

/// Extract a `(kind: <value>)` parenthetical from an operation title or bullet name.
///
/// Returns the cleaned name (with the parenthetical stripped) and the extracted
/// kind value if present. The marker is recognised as `(kind: <value>)` with
/// optional whitespace; the parenthetical must appear at the end of the input.
///
/// Examples:
/// - `"with_currency"` → (`"with_currency"`, `None`)
/// - `"with_currency (kind: with)"` → (`"with_currency"`, `Some("with")`)
/// - `"with_currency  ( kind: with )"` → (`"with_currency"`, `Some("with")`)
fn extract_kind_marker(raw: &str) -> (String, Option<String>) {
    let trimmed = raw.trim_end();
    if !trimmed.ends_with(')') {
        return (raw.trim().to_string(), None);
    }
    let Some(open_paren) = trimmed.rfind('(') else {
        return (raw.trim().to_string(), None);
    };
    let inner = &trimmed[open_paren + 1..trimmed.len() - 1];
    let Some(value) = inner.trim().strip_prefix("kind:").map(str::trim) else {
        return (raw.trim().to_string(), None);
    };
    if value.is_empty() {
        return (raw.trim().to_string(), None);
    }
    let prefix = trimmed[..open_paren].trim_end();
    (prefix.to_string(), Some(value.to_string()))
}

// ── Projection parser ─────────────────────────────────────────────

fn parse_projection_draft(info: DraftInfo, content: &str) -> Result<ProjectionDraft> {
    let sections = parse_sections(content);

    // Reject the legacy `## Source` / `## Queries` shape with a clear migration
    // error. Projections now share the context-aligned six-section template.
    if find_section(&sections, "Source").is_some() || find_section(&sections, "Queries").is_some() {
        return Err(anyhow::anyhow!(
            "{}: this projection draft uses the legacy `## Source` / `## Queries` shape, which is no longer supported. \
            Projections are now structurally identical to contexts — same six sections \
            (Purpose, Role Players, Props, Role Methods, Functionalities, Scenarios). \
            Migration: rewrite each `## Source` entry as a `## Role Players` entry, \
            each `## Queries` entry as a `## Functionalities` entry, and add a `## Role Methods` \
            section describing what each role player provides to the projection.",
            info.relative_path
        ));
    }

    let purpose = require_section_text(&info.relative_path, &sections, "Purpose")?;
    let role_players = parse_role_players(&info.relative_path, &sections)?;
    let props = parse_props(&sections);
    let role_methods = parse_role_method_groups(&info.relative_path, &sections)?;
    let functionalities = parse_functionality_subsections(&info.relative_path, &sections)?;
    let scenarios = parse_scenarios(&sections);

    Ok(ProjectionDraft {
        info,
        purpose,
        role_players,
        props,
        role_methods,
        functionalities,
        scenarios,
    })
}

fn reject_mutation_prose(relative_path: &str, sections: &[Section]) -> Result<()> {
    let mutation_signals = [
        "mutate", "modify", "change", "update", "set ", "write", "delete", "remove", "insert",
        "add ", "append", "push",
    ];
    for section in sections {
        if section.title == "Purpose" || section.title == "Queries" {
            let lower = section.body.to_ascii_lowercase();
            for signal in &mutation_signals {
                if lower.contains(signal) {
                    bail!(
                        "Draft validation failed for {}:\n- Projections are read-only but `## {}` contains mutation-shaped prose (\"{}\")",
                        relative_path,
                        section.title,
                        signal.trim()
                    );
                }
            }
        }
    }
    Ok(())
}

// ── Shared: Scenario parser ───────────────────────────────────────

fn parse_scenarios(sections: &[Section]) -> Vec<ScenarioDraft> {
    let Some(section) = find_section(sections, "Scenarios") else {
        return Vec::new();
    };
    let subsections = parse_subsections(&section.body);
    subsections
        .into_iter()
        .filter_map(|sub| {
            let given = extract_labeled_line(&sub.body, "Given");
            let when = extract_labeled_line(&sub.body, "When");
            let then = extract_labeled_line(&sub.body, "Then");
            if given.is_empty() && when.is_empty() && then.is_empty() {
                return None;
            }
            Some(ScenarioDraft {
                name: sub.title,
                given,
                when,
                then,
            })
        })
        .collect()
}

fn extract_labeled_line(body: &str, label: &str) -> String {
    let prefix = format!("{label}:");
    for line in body.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix(&prefix) {
            return rest.trim().to_string();
        }
    }
    String::new()
}

// ── Component parser ─────────────────────────────────────────────

fn parse_component_draft(info: DraftInfo, content: &str) -> Result<ComponentDraft> {
    let sections = parse_sections(content);

    let purpose = require_section_text(&info.relative_path, &sections, "Purpose")?;
    let props = parse_props(&sections);
    let slots = parse_slot_bullets(&sections);
    let events = parse_event_bullets(&sections);
    let variants = parse_variant_bullets(&sections);
    let composition = parse_composition(&sections)?;
    let invariants = bullet_lines(find_section(&sections, "Invariants").map(|s| s.body.as_str()));
    let scenarios = parse_scenarios(&sections);

    Ok(ComponentDraft {
        info,
        purpose,
        props,
        slots,
        events,
        variants,
        composition,
        invariants,
        scenarios,
    })
}

fn parse_slot_bullets(sections: &[Section]) -> Vec<SlotDraft> {
    let Some(section) = find_section(sections, "Slots") else {
        return Vec::new();
    };
    parse_named_bullets(&section.body)
        .into_iter()
        .map(|(name, desc)| SlotDraft {
            name,
            description: desc,
        })
        .collect()
}

fn parse_event_bullets(sections: &[Section]) -> Vec<EventDraft> {
    let Some(section) = find_section(sections, "Events") else {
        return Vec::new();
    };
    parse_named_bullets(&section.body)
        .into_iter()
        .map(|(name, desc)| EventDraft {
            name,
            description: desc,
        })
        .collect()
}

fn parse_variant_bullets(sections: &[Section]) -> Vec<VariantDraft> {
    let Some(section) = find_section(sections, "Variants") else {
        return Vec::new();
    };
    parse_named_bullets(&section.body)
        .into_iter()
        .map(|(name, desc)| VariantDraft {
            name,
            description: desc,
        })
        .collect()
}

fn parse_composition(sections: &[Section]) -> Result<Option<CompositionDraft>> {
    let Some(section) = find_section(sections, "Composition") else {
        return Ok(None);
    };
    let body = section.body.trim();
    if body.is_empty() {
        return Ok(None);
    }

    let mut child_bindings = Vec::new();
    let mut event_bindings = Vec::new();

    for line in body.lines() {
        let trimmed = line.trim();
        let Some(rest) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        else {
            continue;
        };

        // Event binding: "when <child>.<event>: invoke <context>"
        if let Some(when_rest) = rest
            .strip_prefix("when ")
            .or_else(|| rest.strip_prefix("When "))
        {
            if let Some(eb) = parse_event_binding(when_rest) {
                event_bindings.push(eb);
                continue;
            }
        }

        // Child binding: "<child>: <bindings prose>"
        if let Some((child, bindings)) = rest.split_once(':') {
            child_bindings.push(ChildBindingDraft {
                child: child.trim().to_string(),
                bindings: bindings.trim().to_string(),
            });
        }
    }

    Ok(Some(CompositionDraft {
        child_bindings,
        event_bindings,
    }))
}

/// Parse "when <child>.<event>: invoke <context>" or
/// "when `<child>`.`<event>`: invoke `<context>`"
fn parse_event_binding(text: &str) -> Option<EventBindingDraft> {
    // Try backtick format: `child`.`event`: invoke `Context`
    let re_backtick = regex::Regex::new(
        r"^`([^`]+)`\s*\.\s*`([^`]+)`\s*:\s*invoke\s+`([^`]+)`$"
    ).ok()?;
    if let Some(caps) = re_backtick.captures(text.trim()) {
        return Some(EventBindingDraft {
            child: caps[1].to_string(),
            event: caps[2].to_string(),
            context: caps[3].to_string(),
        });
    }

    // Try plain format: child.event: invoke Context
    let re_plain = regex::Regex::new(
        r"^(\w+)\s*\.\s*(\w+)\s*:\s*invoke\s+(\w+)$"
    ).ok()?;
    if let Some(caps) = re_plain.captures(text.trim()) {
        return Some(EventBindingDraft {
            child: caps[1].to_string(),
            event: caps[2].to_string(),
            context: caps[3].to_string(),
        });
    }

    None
}

// ── Brand parser ─────────────────────────────────────────────────

fn parse_brand_draft(info: DraftInfo, content: &str) -> Result<BrandDraft> {
    let sections = parse_sections(content);

    let purpose = require_section_text(&info.relative_path, &sections, "Purpose")?;

    let mut voice = None;
    let mut token_groups = Vec::new();
    let mut prose_sections = Vec::new();

    // Token groups are subsections under Visual Identity, plus Motion
    if let Some(visual) = find_section(&sections, "Visual Identity") {
        let subsections = parse_subsections(&visual.body);
        for sub in subsections {
            let tokens = parse_named_bullets(&sub.body)
                .into_iter()
                .map(|(name, role)| BrandTokenDraft { name, role })
                .collect();
            token_groups.push(BrandTokenGroupDraft {
                group: sub.title.to_ascii_lowercase(),
                tokens,
            });
        }
    }

    // Motion tokens
    if let Some(motion) = find_section(&sections, "Motion") {
        let tokens = parse_named_bullets(&motion.body)
            .into_iter()
            .map(|(name, role)| BrandTokenDraft { name, role })
            .collect();
        token_groups.push(BrandTokenGroupDraft {
            group: "motion".to_string(),
            tokens,
        });
    }

    // Voice section (prose)
    if let Some(voice_section) = find_section(&sections, "Voice") {
        voice = Some(BrandProseSectionDraft {
            name: "voice".to_string(),
            body: voice_section.body.trim().to_string(),
        });
    }

    // Prose sections: Imagery, Iconography, Logo
    for name in &["Imagery", "Iconography", "Logo"] {
        if let Some(section) = find_section(&sections, name) {
            prose_sections.push(BrandProseSectionDraft {
                name: name.to_ascii_lowercase(),
                body: section.body.trim().to_string(),
            });
        }
    }

    Ok(BrandDraft {
        info,
        purpose,
        voice,
        token_groups,
        prose_sections,
    })
}

// ── Routing table parser ─────────────────────────────────────────

fn parse_routing_table_draft(info: DraftInfo, content: &str) -> Result<RoutingTableDraft> {
    let sections = parse_sections(content);

    let purpose = require_section_text(&info.relative_path, &sections, "Purpose")?;
    let routes = parse_route_bullets(&info.relative_path, &sections)?;

    Ok(RoutingTableDraft {
        info,
        purpose,
        routes,
    })
}

fn parse_route_bullets(relative_path: &str, sections: &[Section]) -> Result<Vec<RouteDraft>> {
    let section = require_section(relative_path, sections, "Routes")?;
    let body = section.body.trim();

    // Detect map-form attempts: first non-blank line starts with a path token
    // followed by `:` (e.g. `/:` or `/login:` or `/products/<id>:`)
    detect_map_form_routes(body)?;

    // The routing table format is:
    // - path: /foo
    //   component: Bar
    //   params:
    //     name: Type
    //
    // Each route starts with "- path:" at the lowest indent level.
    // Everything else at higher indent belongs to that route.

    let mut routes = Vec::new();
    let mut current_path: Option<String> = None;
    let mut current_component: Option<String> = None;
    let mut current_params: Vec<RouteParamDraft> = Vec::new();
    let mut in_params = false;

    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Top-level route entry: starts with "- path:"
        if let Some(rest) = trimmed.strip_prefix("- path:").or_else(|| trimmed.strip_prefix("* path:")) {
            // Flush previous route
            if let (Some(path), Some(component)) =
                (current_path.take(), current_component.take())
            {
                routes.push(RouteDraft {
                    path,
                    component,
                    params: std::mem::take(&mut current_params),
                });
            }
            in_params = false;
            current_path = Some(rest.trim().to_string());
            continue;
        }

        // Nested fields under a route
        if let Some(value) = strip_label(trimmed, "component") {
            current_component = Some(value.to_string());
            in_params = false;
            continue;
        }

        if trimmed.eq_ignore_ascii_case("params:") || trimmed.starts_with("params:") {
            in_params = true;
            continue;
        }

        if in_params {
            // Param line: "name: Type" (possibly with "- " prefix)
            let param_text = trimmed
                .strip_prefix("- ")
                .or_else(|| trimmed.strip_prefix("* "))
                .unwrap_or(trimmed);
            if let Some((name, data_type)) = param_text.split_once(':') {
                current_params.push(RouteParamDraft {
                    name: name.trim().to_string(),
                    data_type: data_type.trim().to_string(),
                });
            }
        }
    }

    // Flush last route
    if let (Some(path), Some(component)) = (current_path, current_component) {
        routes.push(RouteDraft {
            path,
            component,
            params: current_params,
        });
    }

    Ok(routes)
}

/// Detect if the routing-table body uses the map form (path-as-key) instead
/// of the required list form. Fails with a targeted error message.
fn detect_map_form_routes(body: &str) -> Result<()> {
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // Map form: first non-blank line starts with `/` and contains `:`
        // e.g. `/:`, `/login:`, `/products/<id>:`
        if trimmed.starts_with('/') && trimmed.contains(':') {
            bail!(
                "Routes must be a list of entries, each starting with `- path: <url>`. \
                 The map form (`/: {{ component: ... }}`) is not supported — \
                 see the reen-draft-format skill for the routing-table template."
            );
        }
        // Only check the first non-blank line
        break;
    }
    Ok(())
}

// ── App draft parser ─────────────────────────────────────────────

const VALID_APP_KINDS: &[&str] = &["cli_app", "tui_app", "web_app"];

fn parse_app_draft(info: DraftInfo, content: &str) -> Result<AppDraft> {
    let sections = parse_sections(content);

    // Application Kind — required
    let app_kind_section = find_section(&sections, "Application Kind");
    let application_kind = if let Some(section) = app_kind_section {
        let kind = section.body.trim().trim_matches('`').trim().to_string();
        if kind.is_empty() {
            bail!(
                "Draft validation failed for {}:\n- `## Application Kind` section is empty. \
                 Allowed values: {}",
                info.relative_path,
                VALID_APP_KINDS.join(", ")
            );
        }
        if !VALID_APP_KINDS.contains(&kind.as_str()) {
            bail!(
                "Draft validation failed for {}:\n- `Application Kind: {}` is not recognised. \
                 Allowed values: {}",
                info.relative_path,
                kind,
                VALID_APP_KINDS.join(", ")
            );
        }
        kind
    } else {
        bail!(
            "Draft validation failed for {}:\n- Missing required `## Application Kind` section. \
             Allowed values: {}",
            info.relative_path,
            VALID_APP_KINDS.join(", ")
        );
    };

    // Entry Context — optional in draft (resolved during prepare)
    let entry_context = find_section(&sections, "Entry Context")
        .map(|s| s.body.trim().trim_matches('`').trim().to_string())
        .filter(|s| !s.is_empty());

    // Entry Functionality — optional
    let entry_functionality = find_section(&sections, "Entry Functionality")
        .map(|s| s.body.trim().trim_matches('`').trim().to_string())
        .filter(|s| !s.is_empty());

    // Dependencies — optional list
    let dependencies = find_section(&sections, "Dependencies")
        .map(|s| {
            bullet_lines(Some(&s.body))
                .into_iter()
                .filter(|d| !d.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    // Everything else is preserved as notes
    let structured_sections = [
        "Application Kind",
        "Entry Context",
        "Entry Functionality",
        "Dependencies",
    ];
    let mut notes = String::new();
    for section in &sections {
        if !structured_sections.contains(&section.title.as_str()) {
            if !notes.is_empty() {
                notes.push_str("\n\n");
            }
            notes.push_str(&format!("## {}\n\n{}", section.title, section.body));
        }
    }

    Ok(AppDraft {
        info,
        application_kind,
        entry_context,
        entry_functionality,
        dependencies,
        notes: notes.trim().to_string(),
    })
}

// ── Boundary draft parser ────────────────────────────────────────

/// Parse a boundary draft (`drafts/apis/<name>.md`).
///
/// Boundary drafts are not DCI artifacts — they describe an external API boundary.
/// Sections: Purpose (or Description), Source, Operations, Notes.
pub fn parse_boundary_draft(api_name: &str, content: &str) -> Result<BoundaryDraft> {
    let sections = parse_sections(content);

    let purpose = find_section(&sections, "Purpose")
        .or_else(|| find_section(&sections, "Description"))
        .map(|s| s.body.trim().to_string())
        .unwrap_or_default();
    if purpose.is_empty() {
        bail!(
            "Boundary draft for `{}` must have a ## Purpose or ## Description section",
            api_name
        );
    }

    // Source section — may be called "Source" or "Authoritative Sources"
    let source_section = find_section(&sections, "Source")
        .or_else(|| find_section(&sections, "Authoritative Sources"));
    let source = if let Some(s) = source_section {
        parse_boundary_source(&s.body)
    } else {
        BoundarySource {
            openapi_local: None,
            openapi_url: None,
            documentation_url: None,
        }
    };

    // Operations whitelist — may be called "Operations" or "Consumed Surface"
    let ops_section = find_section(&sections, "Operations")
        .or_else(|| find_section(&sections, "Consumed Surface"));
    let operations_whitelist = if let Some(s) = ops_section {
        parse_boundary_operations(&s.body)
    } else {
        Vec::new()
    };

    let notes = find_section(&sections, "Notes")
        .map(|s| bullet_lines(Some(&s.body)))
        .unwrap_or_default();

    Ok(BoundaryDraft {
        api_name: api_name.to_string(),
        purpose,
        source,
        operations_whitelist,
        notes,
    })
}

fn parse_boundary_source(body: &str) -> BoundarySource {
    let mut openapi_local = None;
    let mut openapi_url = None;
    let mut documentation_url = None;
    for line in body.lines() {
        let trimmed = line.trim();
        let rest = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .unwrap_or(trimmed);
        if let Some(v) = strip_label(rest, "OpenAPI Local") {
            openapi_local = Some(v.to_string());
        } else if let Some(v) = strip_label(rest, "OpenAPI URL") {
            openapi_url = Some(v.to_string());
        } else if let Some(v) = strip_label(rest, "Documentation URL") {
            documentation_url = Some(v.to_string());
        } else if let Some(v) = strip_label(rest, "Documentation") {
            documentation_url = Some(v.to_string());
        }
    }
    BoundarySource {
        openapi_local,
        openapi_url,
        documentation_url,
    }
}

fn strip_label<'a>(line: &'a str, label: &str) -> Option<&'a str> {
    let lower = line.to_ascii_lowercase();
    let label_lower = label.to_ascii_lowercase();
    if lower.starts_with(&label_lower) {
        let rest = &line[label.len()..];
        let rest = rest.trim_start_matches(':').trim();
        if rest.is_empty() { None } else { Some(rest) }
    } else {
        None
    }
}

fn parse_boundary_operations(body: &str) -> Vec<String> {
    // Accept bullet lists where items may be just names or "name: description"
    // Also look for known labels like "Schema Types:" or "Message Families:"
    let mut ops = Vec::new();
    for line in body.lines() {
        let trimmed = line.trim();
        let rest = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "));
        if let Some(rest) = rest {
            let lower = rest.to_ascii_lowercase();
            // Skip non-operation labels (URLs, stream endpoints, etc.)
            if lower.starts_with("websocket") {
                continue;
            }
            // Extract comma-separated names from label-style bullets
            if lower.starts_with("schema types")
                || lower.starts_with("message families")
            {
                if let Some((_label, values)) = rest.split_once(':') {
                    for name in values.split(',') {
                        let name = name.trim();
                        if !name.is_empty() {
                            ops.push(name.to_string());
                        }
                    }
                }
                continue;
            }
            // Simple name or "name: description"
            let name = if let Some((n, _)) = rest.split_once(':') {
                n.trim()
            } else {
                rest.trim()
            };
            if !name.is_empty() {
                ops.push(name.to_string());
            }
        }
    }
    ops
}

// ── Markdown helpers ──────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
struct Section {
    title: String,
    body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Subsection {
    title: String,
    body: String,
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
            current_title = Some(title.trim().to_string());
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

fn parse_named_bullets(body: &str) -> Vec<(String, String)> {
    let mut items = Vec::new();
    let mut current: Option<(String, String)> = None;
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(rest) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        {
            if let Some(existing) = current.take() {
                items.push(existing);
            }
            if let Some((name, desc)) = rest.split_once(':') {
                current = Some((name.trim().to_string(), desc.trim().to_string()));
            } else {
                current = Some((rest.trim().to_string(), String::new()));
            }
            continue;
        }
        // Continuation line
        if let Some((_, desc)) = current.as_mut() {
            if !desc.is_empty() {
                desc.push(' ');
            }
            desc.push_str(trimmed);
        }
    }
    if let Some(existing) = current {
        items.push(existing);
    }
    items
}

fn bullet_lines(body: Option<&str>) -> Vec<String> {
    body.into_iter()
        .flat_map(|v| v.lines())
        .filter_map(|line| {
            let trimmed = line.trim();
            trimmed
                .strip_prefix("- ")
                .or_else(|| trimmed.strip_prefix("* "))
                .map(|v| v.trim().to_string())
        })
        .filter(|v| !v.is_empty())
        .collect()
}

fn find_section<'a>(sections: &'a [Section], title: &str) -> Option<&'a Section> {
    sections.iter().find(|s| s.title == title)
}

fn require_section<'a>(
    relative_path: &str,
    sections: &'a [Section],
    title: &str,
) -> Result<&'a Section> {
    find_section(sections, title).ok_or_else(|| {
        anyhow::anyhow!(
            "Draft validation failed for {}:\n- Missing required `## {}` section",
            relative_path,
            title
        )
    })
}

fn require_section_text(relative_path: &str, sections: &[Section], title: &str) -> Result<String> {
    let section = require_section(relative_path, sections, title)?;
    let text = section.body.trim().to_string();
    if text.is_empty() {
        bail!(
            "Draft validation failed for {}:\n- `## {}` section is empty",
            relative_path,
            title
        );
    }
    Ok(text)
}

fn extract_title(content: &str) -> Option<String> {
    content.lines().find_map(|line| {
        line.trim()
            .strip_prefix("# ")
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    })
}

fn first_component(path: &Path) -> Option<&str> {
    match path.components().next()? {
        Component::Normal(v) => v.to_str(),
        _ => None,
    }
}

// ── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn parse_minimal_context_draft() {
        let content = r#"# Context: MoneyTransfer

## Purpose
Enacts a transfer of money between two accounts.

## Role Players
- source: Account — the account money is withdrawn from
- sink: Account — the account money is deposited into

## Props
- amount: the quantity of money to transfer

## Role Methods
### source
- withdraw: removes the transfer amount from the source balance

### sink
- deposit: adds the transfer amount to the sink balance

## Functionalities
### transfer
The source withdraws the amount via its withdraw role method,
then the sink deposits the amount via its deposit role method.

## Scenarios
### successful transfer
Given: source has balance 100, sink has balance 0, amount is 30
When: transfer is invoked
Then: source has balance 70, sink has balance 30
"#;
        let doc = parse_draft_file(
            Path::new("drafts/contexts/money_transfer.md"),
            Path::new("drafts"),
            content,
        )
        .unwrap();

        let DraftDocument::Context(ctx) = doc else {
            panic!("expected Context draft");
        };
        assert_eq!(ctx.info.title, "Context: MoneyTransfer");
        assert_eq!(ctx.role_players.len(), 2);
        assert_eq!(ctx.role_players[0].name, "source");
        assert_eq!(ctx.role_players[0].data_type, "Account");
        assert_eq!(ctx.role_methods.len(), 2);
        assert_eq!(ctx.role_methods[0].role, "source");
        assert_eq!(ctx.role_methods[0].methods[0].name, "withdraw");
        assert_eq!(ctx.functionalities.len(), 1);
        assert_eq!(ctx.scenarios.len(), 1);
        assert_eq!(ctx.scenarios[0].name, "successful transfer");
        assert!(ctx.scenarios[0].given.contains("source has balance 100"));
    }

    #[test]
    fn parse_minimal_data_draft() {
        let content = r#"# Data: Amount

## Purpose
Represents a monetary amount that is never negative.

## Attributes
- value: the numeric quantity in the smallest currency unit

## Invariants
- an Amount is never negative

## Operations
### add
Adds two amounts together.

## Scenarios
### adding two amounts
Given: an amount of 30 and an amount of 50
When: add is invoked
Then: the result is 80
"#;
        let doc = parse_draft_file(
            Path::new("drafts/data/amount.md"),
            Path::new("drafts"),
            content,
        )
        .unwrap();

        let DraftDocument::Data(data) = doc else {
            panic!("expected Data draft");
        };
        assert_eq!(data.attributes.len(), 1);
        assert_eq!(data.attributes[0].name, "value");
        assert_eq!(data.invariants, vec!["an Amount is never negative"]);
        assert_eq!(data.operations.len(), 1);
        assert_eq!(data.scenarios.len(), 1);
    }

    #[test]
    fn extract_kind_marker_handles_present_and_absent() {
        // Absent — name unchanged, kind None.
        assert_eq!(
            extract_kind_marker("with_currency"),
            ("with_currency".to_string(), None)
        );

        // Standard form.
        assert_eq!(
            extract_kind_marker("with_currency (kind: with)"),
            ("with_currency".to_string(), Some("with".to_string()))
        );

        // Whitespace tolerance inside the parenthetical.
        assert_eq!(
            extract_kind_marker("with_currency  ( kind:   with )"),
            ("with_currency".to_string(), Some("with".to_string()))
        );

        // Trailing parenthetical without `kind:` prefix is not a marker.
        assert_eq!(
            extract_kind_marker("withdraw (deprecated)"),
            ("withdraw (deprecated)".to_string(), None)
        );

        // Empty kind value is rejected.
        assert_eq!(
            extract_kind_marker("withdraw (kind:)"),
            ("withdraw (kind:)".to_string(), None)
        );
    }

    #[test]
    fn parse_data_draft_extracts_kind_with_marker() {
        let content = r#"# Data: Money

## Purpose
A monetary amount.

## Attributes
- amount: the value in minor units
- currency: the ISO code

## Invariants
- amount is never negative

## Operations
### with_currency (kind: with)
Returns a new Money with the currency replaced.

### normalise
Adjusts the amount to standard form.

## Scenarios
### example
Given: a Money of 100 DKK
When: with_currency is called with EUR
Then: a Money of 100 EUR
"#;
        let doc = parse_draft_file(
            Path::new("drafts/data/money.md"),
            Path::new("drafts"),
            content,
        )
        .unwrap();

        let DraftDocument::Data(data) = doc else {
            panic!("expected Data draft");
        };
        assert_eq!(data.operations.len(), 2);
        assert_eq!(data.operations[0].name, "with_currency");
        assert_eq!(data.operations[0].kind, Some("with".to_string()));
        assert_eq!(data.operations[1].name, "normalise");
        assert_eq!(data.operations[1].kind, None);
    }

    #[test]
    fn parse_data_draft_extracts_variants_from_h3_sub_sections() {
        // Bug 1 long-term regression: variant data types model sum types via
        // `### <variant> attributes` H3 sub-sections under `## Attributes`.
        let content = r#"# Data: ChatTurn

## Purpose
A turn in the chat conversation.

## Attributes
- kind: the variant classification — one of "user-utterance", "agent-response"
- timestamp: the moment the turn was recorded

### User-utterance attributes
- text: the customer's message

### Agent-response attributes
- text: the agent's reply

### Tool-call attributes
- tool name: the tool being invoked
- tool input: the tool input
"#;
        let doc = parse_draft_file(
            Path::new("drafts/data/chat_turn.md"),
            Path::new("drafts"),
            content,
        )
        .unwrap();
        let DraftDocument::Data(data) = doc else {
            panic!("expected Data draft");
        };

        // Top-level (shared) attributes are kind + timestamp; variant
        // attributes don't leak into the top-level list.
        assert_eq!(data.attributes.len(), 2, "shared attributes only, got {:?}", data.attributes);
        assert_eq!(data.attributes[0].name, "kind");
        assert_eq!(data.attributes[1].name, "timestamp");

        // Three variants in source order.
        assert_eq!(data.variants.len(), 3, "expected three variants");
        assert_eq!(data.variants[0].name, "user_utterance");
        assert_eq!(data.variants[0].attributes.len(), 1);
        assert_eq!(data.variants[0].attributes[0].name, "text");
        assert_eq!(data.variants[1].name, "agent_response");
        assert_eq!(data.variants[1].attributes[0].name, "text");
        assert_eq!(data.variants[2].name, "tool_call");
        assert_eq!(data.variants[2].attributes.len(), 2);
        assert_eq!(data.variants[2].attributes[0].name, "tool name");
        assert_eq!(data.variants[2].attributes[1].name, "tool input");
    }

    #[test]
    fn parse_data_draft_keeps_top_level_bullets_as_shared_attributes() {
        let content = r#"# Data: TransferRequest

## Purpose
A request to transfer money.

## Attributes
- kind: variant classification
- source account: the originating account

### Own-transfer attributes
- destination account: the destination of the transfer

### Domestic-transfer attributes
- recipient name: the name of the receiving party
"#;
        let doc = parse_draft_file(
            Path::new("drafts/data/transfer_request.md"),
            Path::new("drafts"),
            content,
        )
        .unwrap();
        let DraftDocument::Data(data) = doc else {
            panic!("expected Data draft");
        };

        // Shared attributes survive — they're not absorbed into a variant.
        let shared_names: Vec<&str> = data.attributes.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(shared_names, vec!["kind", "source account"]);
        // Per-variant bullets sit under their variant.
        assert_eq!(data.variants.len(), 2);
        assert_eq!(data.variants[0].name, "own_transfer");
        assert_eq!(data.variants[1].name, "domestic_transfer");
    }

    #[test]
    fn parse_data_draft_no_variants_unchanged() {
        // Regression-guard: a Money-style draft (no H3 inside Attributes)
        // still parses with empty `variants` and the existing attribute set.
        let content = r#"# Data: Money

## Purpose
A monetary amount.

## Attributes
- amount: the value in minor units
- currency: the ISO code
"#;
        let doc = parse_draft_file(
            Path::new("drafts/data/money.md"),
            Path::new("drafts"),
            content,
        )
        .unwrap();
        let DraftDocument::Data(data) = doc else {
            panic!("expected Data draft");
        };
        assert_eq!(data.attributes.len(), 2);
        assert!(data.variants.is_empty(), "no variants for plain data, got {:?}", data.variants);
    }

    #[test]
    fn canonicalise_variant_name_normalises_dashes_spaces_and_suffix() {
        assert_eq!(canonicalise_variant_name("User-utterance attributes"), "user_utterance");
        assert_eq!(canonicalise_variant_name("Tool call attributes"), "tool_call");
        assert_eq!(canonicalise_variant_name("Own-transfer"), "own_transfer");
        assert_eq!(canonicalise_variant_name("Confirmation-card-rejected attributes"), "confirmation_card_rejected");
    }

    #[test]
    fn parse_minimal_projection_draft() {
        // New shape: projections share the context-aligned six-section template.
        let content = r#"# Projection: AccountBalance

## Purpose
Provides a read-only view of an account's current balance.

## Role Players
- account: Account — the account being viewed

## Role Methods
### account
- read_balance: provides the current balance value

## Functionalities
### current_balance
Returns the current balance of the underlying account.

## Scenarios
### balance after deposit
Given: an account with balance 100
When: current_balance is queried
Then: returns 100
"#;
        let doc = parse_draft_file(
            Path::new("drafts/projections/account_balance.md"),
            Path::new("drafts"),
            content,
        )
        .unwrap();

        let DraftDocument::Projection(proj) = doc else {
            panic!("expected Projection draft");
        };
        assert_eq!(proj.role_players.len(), 1);
        assert_eq!(proj.role_players[0].name, "account");
        assert_eq!(proj.role_players[0].data_type, "Account");
        assert_eq!(proj.role_methods.len(), 1);
        assert_eq!(proj.role_methods[0].role, "account");
        assert_eq!(proj.role_methods[0].methods[0].name, "read_balance");
        assert_eq!(proj.functionalities.len(), 1);
        assert_eq!(proj.functionalities[0].name, "current_balance");
        assert_eq!(proj.scenarios.len(), 1);
    }

    #[test]
    fn legacy_projection_shape_rejected_with_migration_error() {
        // Old `## Source` / `## Queries` shape must surface a migration error
        // pointing at the new context-aligned template.
        let content = r#"# Projection: LegacyView

## Purpose
A projection that still uses the old shape.

## Source
- Account

## Queries
### balance
Returns the current balance.

## Scenarios
### example
Given: x
When: y
Then: z
"#;
        let result = parse_draft_file(
            Path::new("drafts/projections/legacy.md"),
            Path::new("drafts"),
            content,
        );
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("legacy"), "expected migration error, got: {}", err);
        assert!(err.contains("Role Players"), "expected migration guidance to mention Role Players, got: {}", err);
        assert!(err.contains("Functionalities"), "expected migration guidance to mention Functionalities, got: {}", err);
    }

    #[test]
    fn parse_boundary_draft_aisstream() {
        let content = r#"# AISStream - Real-time AIS Vessel Tracking

## Purpose
AISStream provides real-time global maritime vessel tracking via WebSocket.

## Source
- OpenAPI Local: ../../../shared/openapi/aisstream.yaml
- OpenAPI URL: https://example.com/aisstream.yaml
- Documentation URL: https://aisstream.io/documentation

## Operations
- Subscribe: send a subscription message to begin receiving data
- Receive: receive AIS messages from the stream

## Notes
- The stream uses WebSocket transport
- Authentication is via API key in the subscription message
"#;
        let draft = parse_boundary_draft("aisstream", content).unwrap();
        assert_eq!(draft.api_name, "aisstream");
        assert!(draft.purpose.contains("real-time"));
        assert_eq!(
            draft.source.openapi_local.as_deref(),
            Some("../../../shared/openapi/aisstream.yaml")
        );
        assert!(draft.source.openapi_url.is_some());
        assert!(draft.source.documentation_url.is_some());
        assert_eq!(draft.operations_whitelist, vec!["Subscribe", "Receive"]);
        assert_eq!(draft.notes.len(), 2);
    }

    #[test]
    fn parse_boundary_draft_with_consumed_surface() {
        let content = r#"# AISStream

## Description
Vessel tracking.

## Authoritative Sources
- OpenAPI Local: ./aisstream.yaml

## Consumed Surface
- WebSocket Streams: wss://stream.aisstream.io/v0/stream
- Schema Types: PositionReport, StandardClassBPositionReport

## Notes
- Uses WebSocket
"#;
        let draft = parse_boundary_draft("aisstream", content).unwrap();
        assert!(draft.purpose.contains("Vessel tracking"));
        assert_eq!(
            draft.operations_whitelist,
            vec!["PositionReport", "StandardClassBPositionReport"]
        );
    }

    #[test]
    fn boundary_draft_requires_purpose() {
        let content = "# API\n\n## Notes\n- nothing\n";
        let result = parse_boundary_draft("test", content);
        assert!(result.is_err());
    }

    // ── Component parser tests ───────────────────────────────────

    #[test]
    fn parse_leaf_component_draft() {
        let content = r#"# Component: Button

## Purpose
A pressable element that triggers an action.

## Props
- label: the text shown on the button
- disabled: whether the button ignores presses

## Events
- pressed: the user has pressed the button

## Variants
- primary: the main call-to-action variant
- secondary: a less prominent variant

## Invariants
- a disabled button never fires the pressed event

## Scenarios
### pressing a primary button
Given: a primary button with label "Submit"
When: the user presses the button
Then: the pressed event fires
"#;
        let doc = parse_draft_file(
            Path::new("drafts/components/Button.md"),
            Path::new("drafts"),
            content,
        )
        .unwrap();

        let DraftDocument::Component(comp) = doc else {
            panic!("expected Component draft");
        };
        assert_eq!(comp.info.title, "Component: Button");
        assert_eq!(comp.props.len(), 2);
        assert_eq!(comp.props[0].name, "label");
        assert_eq!(comp.events.len(), 1);
        assert_eq!(comp.events[0].name, "pressed");
        assert_eq!(comp.variants.len(), 2);
        assert!(comp.composition.is_none());
        assert_eq!(comp.invariants.len(), 1);
        assert_eq!(comp.scenarios.len(), 1);
    }

    #[test]
    fn parse_coordinating_component_draft() {
        let content = r#"# Component: LoginForm

## Purpose
Lets the user enter credentials and submit them for authentication.

## Props
- error_message: a text value shown when authentication fails

## Events
- submitted: the user has submitted their credentials

## Composition
- EmailInput: receives the current email value
- PasswordInput: receives the current password value
- SubmitButton: labelled "Sign In"
- when `SubmitButton`.`pressed`: invoke `AuthenticateUser`

## Scenarios
### successful submission
Given: the user has entered valid credentials
When: the submit button is pressed
Then: the submitted event fires with the credentials
"#;
        let doc = parse_draft_file(
            Path::new("drafts/components/LoginForm.md"),
            Path::new("drafts"),
            content,
        )
        .unwrap();

        let DraftDocument::Component(comp) = doc else {
            panic!("expected Component draft");
        };
        assert_eq!(comp.info.title, "Component: LoginForm");
        let composition = comp.composition.as_ref().expect("expected composition");
        assert_eq!(composition.child_bindings.len(), 3);
        assert_eq!(composition.child_bindings[0].child, "EmailInput");
        assert_eq!(composition.event_bindings.len(), 1);
        assert_eq!(composition.event_bindings[0].child, "SubmitButton");
        assert_eq!(composition.event_bindings[0].event, "pressed");
        assert_eq!(composition.event_bindings[0].context, "AuthenticateUser");
    }

    #[test]
    fn parse_event_binding_plain_format() {
        let content = r#"# Component: Form

## Purpose
A simple form.

## Composition
- when button.clicked: invoke SubmitOrder

## Scenarios
### submit
Given: a filled form
When: button is clicked
Then: order is submitted
"#;
        let doc = parse_draft_file(
            Path::new("drafts/components/Form.md"),
            Path::new("drafts"),
            content,
        )
        .unwrap();

        let DraftDocument::Component(comp) = doc else {
            panic!("expected Component draft");
        };
        let composition = comp.composition.as_ref().expect("expected composition");
        assert_eq!(composition.event_bindings.len(), 1);
        assert_eq!(composition.event_bindings[0].child, "button");
        assert_eq!(composition.event_bindings[0].event, "clicked");
        assert_eq!(composition.event_bindings[0].context, "SubmitOrder");
    }

    // ── Brand parser tests ───────────────────────────────────────

    #[test]
    fn parse_brand_draft() {
        let content = r#"# Brand: Acme

## Purpose
Creates a professional, trustworthy experience for business users.

## Voice
- Tone: confident but not arrogant
- Register: professional
- Words we use: simple, clear, guide
- Words we avoid: synergy, leverage, disrupt

## Visual Identity
### Colours
- primary-action: the colour that draws the user to the main call-to-action
- surface-default: the base background for content areas
### Typography
- heading-large: the most prominent text on a page
- body-default: standard reading text
### Spacing
- inset-tight: padding inside compact interactive elements

## Motion
- transition-default: standard duration for state changes, feels responsive

## Imagery
- Photography: authentic, unstaged, natural lighting

## Logo
- Usage: always use the full wordmark on white backgrounds
- Clear space: at least the height of the "A" in Acme on all sides
"#;
        let doc = parse_draft_file(
            Path::new("drafts/brands/Acme.md"),
            Path::new("drafts"),
            content,
        )
        .unwrap();

        let DraftDocument::Brand(brand) = doc else {
            panic!("expected Brand draft");
        };
        assert_eq!(brand.info.title, "Brand: Acme");
        assert!(brand.voice.is_some());
        assert_eq!(brand.token_groups.len(), 4); // colours, typography, spacing, motion
        assert_eq!(brand.token_groups[0].group, "colours");
        assert_eq!(brand.token_groups[0].tokens.len(), 2);
        assert_eq!(brand.token_groups[0].tokens[0].name, "primary-action");
        assert_eq!(brand.token_groups[3].group, "motion");
        assert_eq!(brand.prose_sections.len(), 2); // imagery, logo
    }

    // ── Routing table parser tests ───────────────────────────────

    #[test]
    fn parse_routing_table_draft() {
        let content = r#"# Routes: main

## Purpose
Maps the primary navigation paths of the application.

## Routes
- path: /
  component: HomeScreen
- path: /products/<id>
  component: ProductScreen
  params:
    id: Product
- path: /orders
  component: OrderListScreen
"#;
        let doc = parse_draft_file(
            Path::new("drafts/routes/main.md"),
            Path::new("drafts"),
            content,
        )
        .unwrap();

        let DraftDocument::RoutingTable(rt) = doc else {
            panic!("expected RoutingTable draft");
        };
        assert_eq!(rt.info.title, "Routes: main");
        assert_eq!(rt.routes.len(), 3);
        assert_eq!(rt.routes[0].path, "/");
        assert_eq!(rt.routes[0].component, "HomeScreen");
        assert!(rt.routes[0].params.is_empty());
        assert_eq!(rt.routes[1].path, "/products/<id>");
        assert_eq!(rt.routes[1].component, "ProductScreen");
        assert_eq!(rt.routes[1].params.len(), 1);
        assert_eq!(rt.routes[1].params[0].name, "id");
        assert_eq!(rt.routes[1].params[0].data_type, "Product");
        assert_eq!(rt.routes[2].path, "/orders");
        assert_eq!(rt.routes[2].component, "OrderListScreen");
    }

    // ── Routing table error UX tests (Phase 9.5) ─────────────────

    #[test]
    fn routing_table_map_form_detected_slash_colon() {
        let content = r#"# Routes: main

## Purpose
Main routes.

## Routes
/: { component: HomeScreen }
/login: { component: LoginScreen }
"#;
        let result = parse_draft_file(
            Path::new("drafts/routes/main.md"),
            Path::new("drafts"),
            content,
        );
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Routes must be a list of entries"),
            "expected targeted error message, got: {}",
            err
        );
        assert!(
            err.contains("map form"),
            "expected map form mention, got: {}",
            err
        );
    }

    #[test]
    fn routing_table_map_form_detected_path_colon() {
        let content = r#"# Routes: main

## Purpose
Main routes.

## Routes
/products/<id>:
  component: ProductScreen
"#;
        let result = parse_draft_file(
            Path::new("drafts/routes/main.md"),
            Path::new("drafts"),
            content,
        );
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Routes must be a list of entries"),
            "expected targeted error message, got: {}",
            err
        );
    }

    #[test]
    fn routing_table_list_form_with_angle_brackets_passes() {
        // The list form with path params including <> should parse cleanly
        let content = r#"# Routes: main

## Purpose
Main routes.

## Routes
- path: /products/<id>
  component: ProductScreen
  params:
    id: Product
"#;
        let result = parse_draft_file(
            Path::new("drafts/routes/main.md"),
            Path::new("drafts"),
            content,
        );
        assert!(result.is_ok(), "list form should parse cleanly: {:?}", result.err());
    }

    // ── App parser tests (Phase 2.5) ─────────────────────────────

    #[test]
    fn parse_app_draft_happy_path() {
        let content = r#"# The primary application

## Application Kind
`cli_app`

## Entry Context
`GameLoop`

## Entry Functionality
`tick`

## Dependencies
- anyhow
- crossterm

## Purpose
A terminal Snake game.
"#;
        let doc = parse_draft_file(
            Path::new("drafts/app.md"),
            Path::new("drafts"),
            content,
        )
        .unwrap();

        let DraftDocument::App(app) = doc else {
            panic!("expected App draft");
        };
        assert_eq!(app.application_kind, "cli_app");
        assert_eq!(app.entry_context.as_deref(), Some("GameLoop"));
        assert_eq!(app.entry_functionality.as_deref(), Some("tick"));
        assert_eq!(app.dependencies, vec!["anyhow", "crossterm"]);
        assert!(app.notes.contains("Purpose"));
    }

    #[test]
    fn parse_app_draft_web_app_accepted() {
        let content = r#"# App

## Application Kind
`web_app`
"#;
        let doc = parse_draft_file(
            Path::new("drafts/app.md"),
            Path::new("drafts"),
            content,
        )
        .unwrap();

        let DraftDocument::App(app) = doc else {
            panic!("expected App draft");
        };
        assert_eq!(app.application_kind, "web_app");
    }

    #[test]
    fn parse_app_draft_unknown_kind_rejected() {
        let content = r#"# App

## Application Kind
`desktop_app`
"#;
        let result = parse_draft_file(
            Path::new("drafts/app.md"),
            Path::new("drafts"),
            content,
        );
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("not recognised"),
            "expected unrecognised kind message, got: {}",
            err
        );
    }

    #[test]
    fn parse_app_draft_missing_kind() {
        let content = r#"# App

## Purpose
A test app.
"#;
        let result = parse_draft_file(
            Path::new("drafts/app.md"),
            Path::new("drafts"),
            content,
        );
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("Application Kind"),
            "expected missing Application Kind message, got: {}",
            err
        );
    }

    #[test]
    fn parse_app_draft_tui_app_accepted() {
        let content = r#"# App

## Application Kind
`tui_app`

## Entry Context
`MainMenu`
"#;
        let doc = parse_draft_file(
            Path::new("drafts/app.md"),
            Path::new("drafts"),
            content,
        )
        .unwrap();

        let DraftDocument::App(app) = doc else {
            panic!("expected App draft");
        };
        assert_eq!(app.application_kind, "tui_app");
    }

    #[test]
    fn parse_app_draft_optional_fields() {
        let content = r#"# App

## Application Kind
`cli_app`

## Purpose
Minimal app.
"#;
        let doc = parse_draft_file(
            Path::new("drafts/app.md"),
            Path::new("drafts"),
            content,
        )
        .unwrap();

        let DraftDocument::App(app) = doc else {
            panic!("expected App draft");
        };
        assert!(app.entry_context.is_none());
        assert!(app.entry_functionality.is_none());
        assert!(app.dependencies.is_empty());
    }
}
