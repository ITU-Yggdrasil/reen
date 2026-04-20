use anyhow::{Result, bail};
use std::path::{Component, Path};

// ── Artifact kinds ────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ArtifactKind {
    Data,
    Projection,
    Context,
}

impl ArtifactKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ArtifactKind::Data => "data",
            ArtifactKind::Projection => "projection",
            ArtifactKind::Context => "context",
        }
    }

    pub fn from_draft_path(relative_path: &Path) -> Option<Self> {
        match first_component(relative_path)? {
            "data" => Some(Self::Data),
            "projections" => Some(Self::Projection),
            "contexts" => Some(Self::Context),
            _ => None,
        }
    }

    pub fn from_prepared_path(relative_path: &Path) -> Option<Self> {
        match first_component(relative_path)? {
            "data" => Some(Self::Data),
            "projections" => Some(Self::Projection),
            "contexts" => Some(Self::Context),
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationDraft {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataDraft {
    pub info: DraftInfo,
    pub purpose: String,
    pub attributes: Vec<AttributeDraft>,
    pub invariants: Vec<String>,
    pub operations: Vec<OperationDraft>,
    pub scenarios: Vec<ScenarioDraft>,
}

// ── Projection draft ──────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryDraft {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionDraft {
    pub info: DraftInfo,
    pub purpose: String,
    pub source: String,
    pub queries: Vec<QueryDraft>,
    pub scenarios: Vec<ScenarioDraft>,
}

// ── Top-level enum ────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DraftDocument {
    Data(DataDraft),
    Projection(ProjectionDraft),
    Context(ContextDraft),
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
    })
}

fn parse_attribute_bullets(sections: &[Section]) -> Vec<AttributeDraft> {
    let Some(section) = find_section(sections, "Attributes") else {
        return Vec::new();
    };
    parse_named_bullets(&section.body)
        .into_iter()
        .map(|(name, desc)| AttributeDraft {
            name,
            description: desc,
        })
        .collect()
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
            .map(|(name, desc)| OperationDraft {
                name,
                description: desc,
            })
            .collect();
    }
    subsections
        .into_iter()
        .map(|sub| OperationDraft {
            name: sub.title,
            description: sub.body.trim().to_string(),
        })
        .collect()
}

// ── Projection parser ─────────────────────────────────────────────

fn parse_projection_draft(info: DraftInfo, content: &str) -> Result<ProjectionDraft> {
    let sections = parse_sections(content);

    let purpose = require_section_text(&info.relative_path, &sections, "Purpose")?;
    let source = parse_source_section(&sections);
    let queries = parse_query_subsections(&sections);
    let scenarios = parse_scenarios(&sections);

    // Reject mutation-shaped prose in projections
    reject_mutation_prose(&info.relative_path, &sections)?;

    Ok(ProjectionDraft {
        info,
        purpose,
        source,
        queries,
        scenarios,
    })
}

fn parse_source_section(sections: &[Section]) -> String {
    let Some(section) = find_section(sections, "Source") else {
        return String::new();
    };
    // Source can be a single bullet or plain text
    let lines = bullet_lines(Some(&section.body));
    if !lines.is_empty() {
        return lines.join(", ");
    }
    section.body.trim().to_string()
}

fn parse_query_subsections(sections: &[Section]) -> Vec<QueryDraft> {
    let Some(section) = find_section(sections, "Queries") else {
        return Vec::new();
    };
    let subsections = parse_subsections(&section.body);
    if subsections.is_empty() {
        return parse_named_bullets(&section.body)
            .into_iter()
            .map(|(name, desc)| QueryDraft {
                name,
                description: desc,
            })
            .collect();
    }
    subsections
        .into_iter()
        .map(|sub| QueryDraft {
            name: sub.title,
            description: sub.body.trim().to_string(),
        })
        .collect()
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
    fn parse_minimal_projection_draft() {
        let content = r#"# Projection: AccountBalance

## Purpose
Provides a read-only view of an account's current balance.

## Source
- Account

## Queries
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
        assert_eq!(proj.source, "Account");
        assert_eq!(proj.queries.len(), 1);
        assert_eq!(proj.scenarios.len(), 1);
    }

    #[test]
    fn projection_with_mutation_is_rejected() {
        let content = r#"# Projection: BadProjection

## Purpose
This projection will modify data.

## Source
- Account

## Queries
### update_balance
Updates the balance.

## Scenarios
### test
Given: x
When: y
Then: z
"#;
        let result = parse_draft_file(
            Path::new("drafts/projections/bad.md"),
            Path::new("drafts"),
            content,
        );
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("mutation-shaped prose"));
    }
}
