use anyhow::{anyhow, bail, Context, Result};
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BrandSpecValidation {
    pub blocking_ambiguities: Vec<String>,
    pub defined_tokens: BTreeSet<String>,
}

const TITLE: &str = "Brand Identity Specification";
const REQUIRED_SECTIONS: &[&str] = &[
    "Description",
    "Brand Metadata",
    "Color Tokens",
    "Typography",
    "Spacing System",
    "Iconography",
    "Motion",
    "Layout Principles",
    "Token Reference Rules",
];
const OPTIONAL_SECTIONS: &[&str] = &["Blocking Ambiguities", "Implementation Choices Left Open"];

const COLOR_SUBSECTIONS: &[&str] = &[
    "Primary",
    "Secondary",
    "Semantic",
    "Surface",
    "Text and Foreground/Background",
];
const TYPOGRAPHY_SUBSECTIONS: &[&str] = &[
    "Families",
    "Scales",
    "Weights",
    "Line Heights",
    "Named Text Styles",
];
const SPACING_SUBSECTIONS: &[&str] = &[
    "Base Unit",
    "Scale",
    "Layout Margins",
    "Gutters and Container Spacing",
];
const ICONOGRAPHY_SUBSECTIONS: &[&str] = &["Style", "Size Set", "Usage Constraints"];
const MOTION_SUBSECTIONS: &[&str] = &["Durations", "Easing", "Usage Principles"];
const LAYOUT_SUBSECTIONS: &[&str] = &["Grid", "Breakpoints", "Container Widths and Padding"];

#[derive(Clone, Debug, Default)]
struct Section {
    body: Vec<String>,
    subsections: BTreeMap<String, Vec<String>>,
}

pub fn is_brand_draft_path(path: &Path, drafts_dir: &str) -> bool {
    path.strip_prefix(Path::new(drafts_dir))
        .ok()
        .and_then(|relative| relative.components().next())
        .and_then(|component| component.as_os_str().to_str())
        == Some("brands")
}

pub fn is_brand_spec_path(path: &Path, specifications_dir: &str) -> bool {
    path.strip_prefix(Path::new(specifications_dir))
        .ok()
        .and_then(|relative| relative.components().next())
        .and_then(|component| component.as_os_str().to_str())
        == Some("brands")
}

pub fn validate_brand_spec_content(spec_content: &str) -> Result<BrandSpecValidation> {
    let parsed = parse_brand_spec(spec_content)?;
    let blocking_ambiguities = extract_blocking_ambiguities(
        parsed
            .sections
            .get("Blocking Ambiguities")
            .map(|section| &section.body),
    );
    let defined_tokens = collect_defined_tokens(&parsed.sections);

    Ok(BrandSpecValidation {
        blocking_ambiguities,
        defined_tokens,
    })
}

pub fn collect_brand_token_references(content: &str) -> Vec<String> {
    let pattern = Regex::new(r"\bbrand(?:\.[A-Za-z0-9_-]+)+\b").expect("brand ref regex");
    let mut refs = BTreeSet::new();
    for m in pattern.find_iter(content) {
        refs.insert(m.as_str().to_string());
    }
    refs.into_iter().collect()
}

pub fn unresolved_brand_token_references(
    content: &str,
    specifications_dir: &str,
) -> Result<Vec<String>> {
    let references = collect_brand_token_references(content);
    if references.is_empty() {
        return Ok(Vec::new());
    }

    let defined = collect_defined_brand_tokens(specifications_dir)?;
    Ok(references
        .into_iter()
        .filter(|reference| !defined.contains(reference))
        .collect())
}

fn parse_brand_spec(spec_content: &str) -> Result<ParsedBrandSpec> {
    let mut first_heading_seen = false;
    let mut section_order = Vec::new();
    let mut sections: BTreeMap<String, Section> = BTreeMap::new();
    let mut current_section: Option<String> = None;
    let mut current_subsection: Option<String> = None;

    for line in spec_content.lines() {
        if let Some((level, heading)) = parse_heading(line) {
            match level {
                1 => {
                    if first_heading_seen {
                        bail!("brand specification must contain exactly one level-1 heading");
                    }
                    if heading != TITLE {
                        bail!("brand specification title must be '# {}'", TITLE);
                    }
                    first_heading_seen = true;
                    current_section = None;
                    current_subsection = None;
                }
                2 => {
                    ensure_title_seen(first_heading_seen)?;
                    validate_section_name(&heading)?;
                    if sections.contains_key(&heading) {
                        bail!("brand specification section '{}' must appear only once", heading);
                    }
                    section_order.push(heading.clone());
                    sections.insert(heading.clone(), Section::default());
                    current_section = Some(heading);
                    current_subsection = None;
                }
                3 => {
                    ensure_title_seen(first_heading_seen)?;
                    let section_name = current_section
                        .clone()
                        .ok_or_else(|| anyhow!("brand subsection '{}' must appear under a section", heading))?;
                    let expected = required_subsections_for(&section_name);
                    if expected.is_empty() {
                        bail!(
                            "section '{}' does not allow subsection '{}'",
                            section_name,
                            heading
                        );
                    }
                    if !expected.iter().any(|candidate| *candidate == heading) {
                        bail!(
                            "section '{}' has unexpected subsection '{}'",
                            section_name,
                            heading
                        );
                    }
                    let section = sections
                        .get_mut(&section_name)
                        .ok_or_else(|| anyhow!("missing section '{}'", section_name))?;
                    if section.subsections.contains_key(&heading) {
                        bail!(
                            "brand specification subsection '{} / {}' must appear only once",
                            section_name,
                            heading
                        );
                    }
                    section.subsections.insert(heading.clone(), Vec::new());
                    current_subsection = Some(heading);
                }
                _ => {
                    bail!("brand specification may only use heading levels 1-3");
                }
            }
            continue;
        }

        if let Some(section_name) = current_section.as_ref() {
            let section = sections
                .get_mut(section_name)
                .ok_or_else(|| anyhow!("missing section '{}'", section_name))?;
            if let Some(subsection_name) = current_subsection.as_ref() {
                if let Some(lines) = section.subsections.get_mut(subsection_name) {
                    lines.push(line.to_string());
                }
            } else {
                section.body.push(line.to_string());
            }
        } else if !line.trim().is_empty() {
            bail!("content must appear under the title or a named section");
        }
    }

    ensure_title_seen(first_heading_seen)?;
    validate_required_section_order(&section_order)?;
    validate_required_subsections(&sections)?;

    Ok(ParsedBrandSpec { sections })
}

fn ensure_title_seen(first_heading_seen: bool) -> Result<()> {
    if first_heading_seen {
        Ok(())
    } else {
        bail!("brand specification must start with '# {}'", TITLE)
    }
}

fn validate_section_name(name: &str) -> Result<()> {
    if REQUIRED_SECTIONS.iter().any(|candidate| *candidate == name)
        || OPTIONAL_SECTIONS.iter().any(|candidate| *candidate == name)
    {
        Ok(())
    } else {
        bail!("brand specification has unexpected section '{}'", name);
    }
}

fn validate_required_section_order(section_order: &[String]) -> Result<()> {
    for required in REQUIRED_SECTIONS {
        if !section_order.iter().any(|section| section == required) {
            bail!("brand specification section '{}' is required", required);
        }
    }

    let mut positions = HashMap::new();
    for (idx, section) in section_order.iter().enumerate() {
        positions.insert(section.as_str(), idx);
    }

    for pair in REQUIRED_SECTIONS.windows(2) {
        let left = positions
            .get(pair[0])
            .ok_or_else(|| anyhow!("brand specification section '{}' is required", pair[0]))?;
        let right = positions
            .get(pair[1])
            .ok_or_else(|| anyhow!("brand specification section '{}' is required", pair[1]))?;
        if left >= right {
            bail!(
                "brand specification sections must appear in canonical order; '{}' must come before '{}'",
                pair[0],
                pair[1]
            );
        }
    }

    if let Some(blocking) = positions.get("Blocking Ambiguities") {
        let token_rules = positions
            .get("Token Reference Rules")
            .ok_or_else(|| anyhow!("brand specification section 'Token Reference Rules' is required"))?;
        if blocking <= token_rules {
            bail!(
                "brand specification section 'Blocking Ambiguities' must come after 'Token Reference Rules'"
            );
        }
    }

    if let Some(impl_open) = positions.get("Implementation Choices Left Open") {
        let token_rules = positions
            .get("Token Reference Rules")
            .ok_or_else(|| anyhow!("brand specification section 'Token Reference Rules' is required"))?;
        if impl_open <= token_rules {
            bail!(
                "brand specification section 'Implementation Choices Left Open' must come after 'Token Reference Rules'"
            );
        }
        if let Some(blocking) = positions.get("Blocking Ambiguities") {
            if impl_open <= blocking {
                bail!(
                    "brand specification section 'Implementation Choices Left Open' must come after 'Blocking Ambiguities'"
                );
            }
        }
    }

    Ok(())
}

fn validate_required_subsections(sections: &BTreeMap<String, Section>) -> Result<()> {
    for (section_name, required) in [
        ("Color Tokens", COLOR_SUBSECTIONS),
        ("Typography", TYPOGRAPHY_SUBSECTIONS),
        ("Spacing System", SPACING_SUBSECTIONS),
        ("Iconography", ICONOGRAPHY_SUBSECTIONS),
        ("Motion", MOTION_SUBSECTIONS),
        ("Layout Principles", LAYOUT_SUBSECTIONS),
    ] {
        let section = sections
            .get(section_name)
            .ok_or_else(|| anyhow!("brand specification section '{}' is required", section_name))?;
        for subsection in required {
            if !section.subsections.contains_key(*subsection) {
                bail!(
                    "brand specification subsection '{} / {}' is required",
                    section_name,
                    subsection
                );
            }
        }
    }

    Ok(())
}

fn required_subsections_for(section_name: &str) -> &'static [&'static str] {
    match section_name {
        "Color Tokens" => COLOR_SUBSECTIONS,
        "Typography" => TYPOGRAPHY_SUBSECTIONS,
        "Spacing System" => SPACING_SUBSECTIONS,
        "Iconography" => ICONOGRAPHY_SUBSECTIONS,
        "Motion" => MOTION_SUBSECTIONS,
        "Layout Principles" => LAYOUT_SUBSECTIONS,
        _ => &[],
    }
}

fn parse_heading(line: &str) -> Option<(usize, String)> {
    let trimmed = line.trim();
    if !trimmed.starts_with('#') {
        return None;
    }

    let hashes = trimmed.chars().take_while(|c| *c == '#').count();
    let title = trimmed[hashes..].trim();
    if title.is_empty() {
        None
    } else {
        Some((hashes, title.to_string()))
    }
}

fn extract_blocking_ambiguities(lines: Option<&Vec<String>>) -> Vec<String> {
    let Some(lines) = lines else {
        return Vec::new();
    };

    extract_bullets(&lines.join("\n"))
        .into_iter()
        .filter(|item| !is_placeholder_blocker(item))
        .collect()
}

fn extract_bullets(section: &str) -> Vec<String> {
    let mut items = Vec::new();
    for line in section.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("- ") {
            let value = rest.trim();
            if !value.is_empty() {
                items.push(value.to_string());
            }
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("* ") {
            let value = rest.trim();
            if !value.is_empty() {
                items.push(value.to_string());
            }
            continue;
        }

        let mut chars = trimmed.chars();
        let is_numbered = chars.next().map(|c| c.is_ascii_digit()).unwrap_or(false)
            && trimmed.contains(". ");
        if is_numbered {
            if let Some((_, rest)) = trimmed.split_once(". ") {
                let value = rest.trim();
                if !value.is_empty() {
                    items.push(value.to_string());
                }
            }
        }
    }
    items
}

fn is_placeholder_blocker(text: &str) -> bool {
    matches!(
        text.trim().to_ascii_lowercase().as_str(),
        "none" | "no blocking ambiguities" | "n/a"
    )
}

fn collect_defined_tokens(sections: &BTreeMap<String, Section>) -> BTreeSet<String> {
    let token_pattern = Regex::new(r"\bbrand(?:\.[A-Za-z0-9_-]+)+\b").expect("brand token regex");
    let mut defined_tokens = BTreeSet::new();

    for section_name in [
        "Color Tokens",
        "Typography",
        "Spacing System",
        "Iconography",
        "Motion",
        "Layout Principles",
    ] {
        if let Some(section) = sections.get(section_name) {
            collect_tokens_from_lines(&section.body, &token_pattern, &mut defined_tokens);
            for lines in section.subsections.values() {
                collect_tokens_from_lines(lines, &token_pattern, &mut defined_tokens);
            }
        }
    }

    defined_tokens
}

fn collect_tokens_from_lines(lines: &[String], pattern: &Regex, out: &mut BTreeSet<String>) {
    for line in lines {
        let trimmed = line.trim();
        if !(trimmed.starts_with("- ")
            || trimmed.starts_with("* ")
            || trimmed.starts_with('|')
            || trimmed.contains('`'))
        {
            continue;
        }
        for m in pattern.find_iter(trimmed) {
            out.insert(m.as_str().to_string());
        }
    }
}

fn collect_defined_brand_tokens(specifications_dir: &str) -> Result<BTreeSet<String>> {
    let brand_dir = Path::new(specifications_dir).join("brands");
    if !brand_dir.exists() {
        return Ok(BTreeSet::new());
    }

    let mut files = Vec::new();
    collect_markdown_files_recursive(&brand_dir, &mut files)?;
    files.sort();

    let mut token_to_file: HashMap<String, PathBuf> = HashMap::new();
    for file in files {
        let content = fs::read_to_string(&file)
            .with_context(|| format!("failed reading brand specification {}", file.display()))?;
        let validation = validate_brand_spec_content(&content)
            .with_context(|| format!("invalid brand specification {}", file.display()))?;
        for token in validation.defined_tokens {
            if let Some(existing) = token_to_file.insert(token.clone(), file.clone()) {
                bail!(
                    "duplicate brand token '{}' defined in both {} and {}",
                    token,
                    existing.display(),
                    file.display()
                );
            }
        }
    }

    Ok(token_to_file.into_keys().collect())
}

fn collect_markdown_files_recursive(dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    if !dir.exists() || !dir.is_dir() {
        return Ok(());
    }

    for entry in fs::read_dir(dir).with_context(|| format!("failed to read {}", dir.display()))? {
        let path = entry?.path();
        if path.is_dir() {
            collect_markdown_files_recursive(&path, files)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("md") {
            files.push(path);
        }
    }

    Ok(())
}

#[derive(Clone, Debug)]
struct ParsedBrandSpec {
    sections: BTreeMap<String, Section>,
}

#[cfg(test)]
mod tests {
    use super::{
        collect_brand_token_references, unresolved_brand_token_references,
        validate_brand_spec_content,
    };
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(prefix: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time ok")
            .as_nanos();
        std::env::temp_dir().join(format!("reen_brand_specs_{}_{}", prefix, nanos))
    }

    fn valid_brand_spec() -> &'static str {
        r#"# Brand Identity Specification

## Description
Structured visual identity for Acme.

## Brand Metadata
- Brand name: Acme
- Version: 1.0

## Color Tokens
### Primary
- `brand.colors.primary.default`: `#112233`

### Secondary
- `brand.colors.secondary.default`: `#445566`

### Semantic
| Token | Value |
| --- | --- |
| `brand.colors.semantic.success` | `#00aa55` |

### Surface
- `brand.colors.surface.default`: `#ffffff`

### Text and Foreground/Background
- `brand.colors.text.primary`: `#111111`

## Typography
### Families
- `brand.typography.families.primary`: `Fraunces`

### Scales
- `brand.typography.scales.body.medium.size`: `16px`

### Weights
- `brand.typography.weights.regular`: `400`

### Line Heights
- `brand.typography.line_heights.body.medium`: `24px`

### Named Text Styles
- `brand.typography.text_styles.body.medium`: Uses `brand.typography.scales.body.medium.size`

## Spacing System
### Base Unit
- `brand.spacing.base_unit`: `8px`

### Scale
- `brand.spacing.scale.4`: `16px`

### Layout Margins
- `brand.spacing.layout_margins.desktop`: `64px`

### Gutters and Container Spacing
- `brand.spacing.gutters.default`: `16px`

## Iconography
### Style
- `brand.iconography.style.default`: `outlined`

### Size Set
- `brand.iconography.size.medium`: `24px`

### Usage Constraints
- `brand.iconography.usage.actions`: Use for actions and status only.

## Motion
### Durations
- `brand.motion.durations.fast`: `150ms`

### Easing
- `brand.motion.easing.standard`: `cubic-bezier(0.4, 0, 0.2, 1)`

### Usage Principles
- `brand.motion.usage.state_changes`: Use motion to indicate state changes.

## Layout Principles
### Grid
- `brand.layout.grid.columns`: `12`

### Breakpoints
- `brand.layout.breakpoints.md`: `768px`

### Container Widths and Padding
- `brand.layout.container.max_width`: `1200px`

## Token Reference Rules
- Downstream specifications must reference tokens by stable dotted token names such as `brand.colors.primary.default`.

## Implementation Choices Left Open
- Non-blocking: The final design-system package format is left to implementation."#
    }

    #[test]
    fn validates_brand_markdown_spec_and_collects_tokens() {
        let validation = validate_brand_spec_content(valid_brand_spec()).expect("valid brand spec");
        assert!(validation
            .defined_tokens
            .contains("brand.colors.primary.default"));
        assert!(validation
            .defined_tokens
            .contains("brand.typography.families.primary"));
        assert!(validation.blocking_ambiguities.is_empty());
    }

    #[test]
    fn rejects_missing_required_section() {
        let spec = valid_brand_spec().replace("## Token Reference Rules", "## Token Rules");
        let err = validate_brand_spec_content(&spec).expect_err("expected failure");
        assert!(err.to_string().contains("unexpected section 'Token Rules'"));
    }

    #[test]
    fn extracts_blocking_ambiguities_from_markdown_section() {
        let spec = valid_brand_spec().replace(
            "## Implementation Choices Left Open\n- Non-blocking: The final design-system package format is left to implementation.",
            "## Blocking Ambiguities\n- The draft does not define any semantic color tokens.\n\n## Implementation Choices Left Open\n- Non-blocking: The final design-system package format is left to implementation.",
        );
        let validation = validate_brand_spec_content(&spec).expect("valid brand spec");
        assert_eq!(
            validation.blocking_ambiguities,
            vec!["The draft does not define any semantic color tokens.".to_string()]
        );
    }

    #[test]
    fn collects_references_from_markdown() {
        let refs = collect_brand_token_references(
            "Use `brand.colors.primary.default` and brand.spacing.scale.4 in the layout.",
        );
        assert_eq!(
            refs,
            vec![
                "brand.colors.primary.default".to_string(),
                "brand.spacing.scale.4".to_string()
            ]
        );
    }

    #[test]
    fn unresolved_reference_detection_uses_brand_specs_directory() {
        let root = temp_root("refs");
        let specs = root.join("specifications").join("brands");
        fs::create_dir_all(&specs).expect("mkdir");
        fs::write(specs.join("acme.md"), valid_brand_spec()).expect("write");

        let unresolved = unresolved_brand_token_references(
            "brand.colors.primary.default brand.layout.breakpoints.xl",
            root.join("specifications").to_str().expect("spec path"),
        )
        .expect("resolved refs");

        assert_eq!(unresolved, vec!["brand.layout.breakpoints.xl".to_string()]);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn duplicate_token_detection_reports_conflicting_markdown_specs() {
        let root = temp_root("dupe");
        let specs = root.join("specifications").join("brands");
        fs::create_dir_all(&specs).expect("mkdir");
        fs::write(specs.join("acme.md"), valid_brand_spec()).expect("write");
        fs::write(specs.join("beta.md"), valid_brand_spec()).expect("write");

        let err = unresolved_brand_token_references(
            "brand.colors.primary.default",
            root.join("specifications").to_str().expect("spec path"),
        )
        .expect_err("expected duplicate failure");

        assert!(err
            .to_string()
            .contains("duplicate brand token 'brand.colors.primary.default'"));

        let _ = fs::remove_dir_all(root);
    }
}
