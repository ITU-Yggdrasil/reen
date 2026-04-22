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
    "Iconography",
    "Motion",
    "Token Reference Rules",
];
const OPTIONAL_SECTIONS: &[&str] = &[
    "Brand Essence",
    "Audience and Positioning",
    "Verbal Identity",
    "Logo System",
    "Imagery",
    "Composition Principles",
    "Layout Principles",
    "Blocking Ambiguities",
    "Implementation Choices Left Open",
];

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
const ICONOGRAPHY_SUBSECTIONS: &[&str] = &["Style", "Size Set", "Usage Constraints"];
const MOTION_SUBSECTIONS: &[&str] = &["Durations", "Easing", "Usage Principles"];
const BRAND_ESSENCE_SUBSECTIONS: &[&str] = &["Mission", "Vision", "Values"];
const AUDIENCE_SUBSECTIONS: &[&str] = &["Audience", "Positioning"];
const VERBAL_IDENTITY_SUBSECTIONS: &[&str] = &[
    "Personality Attributes",
    "Tone Guidelines",
    "Messaging Do/Don't",
];
const LOGO_SYSTEM_SUBSECTIONS: &[&str] =
    &["Mark Description", "Clear Space and Sizing", "Usage Rules"];
const IMAGERY_SUBSECTIONS: &[&str] = &["Style Attributes", "Subject Guidance", "Avoid"];
const COMPOSITION_SUBSECTIONS: &[&str] = &["Hierarchy", "Density", "Emphasis"];

#[derive(Clone, Debug, Default)]
struct Section {
    body: Vec<String>,
    subsections: BTreeMap<String, Vec<String>>,
}

pub fn is_brand_draft_path(path: &Path, drafts_dir: &str) -> bool {
    matches!(
        path.strip_prefix(Path::new(drafts_dir))
            .ok()
            .and_then(|relative| relative.components().next())
            .and_then(|component| component.as_os_str().to_str()),
        Some("brands" | "visuals")
    )
}

pub fn is_brand_spec_path(path: &Path, specifications_dir: &str) -> bool {
    matches!(
        path.strip_prefix(Path::new(specifications_dir))
            .ok()
            .and_then(|relative| relative.components().next())
            .and_then(|component| component.as_os_str().to_str()),
        Some("brands" | "visuals")
    )
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

/// Returns a list of missing required brand specification sections/subsections.
///
/// This helper is intentionally non-bailing: it is used by CLI flows that should
/// continue processing other items and report all missing requirements at the end.
pub fn missing_required_brand_spec_parts(spec_content: &str) -> Vec<String> {
    let mut missing = Vec::new();

    let mut saw_title = false;
    let mut saw_wrong_title: Option<String> = None;
    let mut sections_seen: BTreeSet<String> = BTreeSet::new();
    let mut subsections_seen: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut current_section: Option<String> = None;

    for line in spec_content.lines() {
        let Some((level, heading)) = parse_heading(line) else {
            continue;
        };

        match level {
            1 => {
                if heading == TITLE {
                    saw_title = true;
                } else if saw_wrong_title.is_none() {
                    saw_wrong_title = Some(heading);
                }
                current_section = None;
            }
            2 => {
                sections_seen.insert(heading.clone());
                current_section = Some(heading);
            }
            3 => {
                if let Some(section) = current_section.as_ref() {
                    subsections_seen
                        .entry(section.clone())
                        .or_default()
                        .insert(heading);
                }
            }
            _ => {}
        }
    }

    if !saw_title {
        if let Some(found) = saw_wrong_title {
            missing.push(format!("Missing title '# {}' (found '# {}')", TITLE, found));
        } else {
            missing.push(format!("Missing title '# {}'", TITLE));
        }
    }

    for required in REQUIRED_SECTIONS {
        if !sections_seen.iter().any(|s| s == required) {
            missing.push(format!("Missing section '## {}'", required));
        }
    }

    for (section_name, required) in [
        ("Color Tokens", COLOR_SUBSECTIONS),
        ("Typography", TYPOGRAPHY_SUBSECTIONS),
        ("Iconography", ICONOGRAPHY_SUBSECTIONS),
        ("Motion", MOTION_SUBSECTIONS),
    ] {
        if !sections_seen.iter().any(|s| s == section_name) {
            continue;
        }
        let seen = subsections_seen.get(section_name);
        for subsection in required {
            let has = seen
                .map(|set| set.iter().any(|s| s == *subsection))
                .unwrap_or(false);
            if !has {
                missing.push(format!(
                    "Missing subsection '### {}' under '## {}'",
                    subsection, section_name
                ));
            }
        }
    }

    missing
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
                        bail!(
                            "brand specification section '{}' must appear only once",
                            heading
                        );
                    }
                    section_order.push(heading.clone());
                    sections.insert(heading.clone(), Section::default());
                    current_section = Some(heading);
                    current_subsection = None;
                }
                3 => {
                    ensure_title_seen(first_heading_seen)?;
                    let section_name = current_section.clone().ok_or_else(|| {
                        anyhow!("brand subsection '{}' must appear under a section", heading)
                    })?;
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

    let mut previous_rank: Option<usize> = None;
    for section in section_order {
        let rank = section_rank(section)?;
        if let Some(previous) = previous_rank {
            if rank < previous {
                bail!(
                    "brand specification sections must appear in canonical order; '{}' is out of place",
                    section
                );
            }
        }
        previous_rank = Some(rank);
    }

    if let Some(blocking) = positions.get("Blocking Ambiguities") {
        let token_rules = positions.get("Token Reference Rules").ok_or_else(|| {
            anyhow!("brand specification section 'Token Reference Rules' is required")
        })?;
        if blocking <= token_rules {
            bail!(
                "brand specification section 'Blocking Ambiguities' must come after 'Token Reference Rules'"
            );
        }
    }

    if let Some(impl_open) = positions.get("Implementation Choices Left Open") {
        let token_rules = positions.get("Token Reference Rules").ok_or_else(|| {
            anyhow!("brand specification section 'Token Reference Rules' is required")
        })?;
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

fn section_rank(section_name: &str) -> Result<usize> {
    match section_name {
        "Description" => Ok(0),
        "Brand Metadata" => Ok(1),
        "Brand Essence" => Ok(2),
        "Audience and Positioning" => Ok(3),
        "Verbal Identity" => Ok(4),
        "Logo System" => Ok(5),
        "Color Tokens" => Ok(6),
        "Typography" => Ok(7),
        "Imagery" => Ok(8),
        "Iconography" => Ok(9),
        "Motion" => Ok(10),
        "Composition Principles" => Ok(11),
        "Layout Principles" => Ok(12),
        "Token Reference Rules" => Ok(13),
        "Blocking Ambiguities" => Ok(14),
        "Implementation Choices Left Open" => Ok(15),
        _ => bail!(
            "brand specification has unexpected section '{}'",
            section_name
        ),
    }
}

fn validate_required_subsections(sections: &BTreeMap<String, Section>) -> Result<()> {
    for (section_name, required) in [
        ("Color Tokens", COLOR_SUBSECTIONS),
        ("Typography", TYPOGRAPHY_SUBSECTIONS),
        ("Iconography", ICONOGRAPHY_SUBSECTIONS),
        ("Motion", MOTION_SUBSECTIONS),
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
        "Brand Essence" => BRAND_ESSENCE_SUBSECTIONS,
        "Audience and Positioning" => AUDIENCE_SUBSECTIONS,
        "Verbal Identity" => VERBAL_IDENTITY_SUBSECTIONS,
        "Logo System" => LOGO_SYSTEM_SUBSECTIONS,
        "Color Tokens" => COLOR_SUBSECTIONS,
        "Typography" => TYPOGRAPHY_SUBSECTIONS,
        "Imagery" => IMAGERY_SUBSECTIONS,
        "Iconography" => ICONOGRAPHY_SUBSECTIONS,
        "Motion" => MOTION_SUBSECTIONS,
        "Composition Principles" => COMPOSITION_SUBSECTIONS,
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
        let is_numbered =
            chars.next().map(|c| c.is_ascii_digit()).unwrap_or(false) && trimmed.contains(". ");
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
    let normalized = text
        .trim()
        .trim_start_matches('-')
        .trim_start_matches('*')
        .trim()
        .trim_end_matches('.')
        .to_ascii_lowercase();
    matches!(
        normalized.as_str(),
        "none" | "no blocking ambiguities" | "n/a"
    )
} 

fn collect_defined_tokens(sections: &BTreeMap<String, Section>) -> BTreeSet<String> {
    let token_pattern = Regex::new(r"\bbrand(?:\.[A-Za-z0-9_-]+)+\b").expect("brand token regex");
    let mut defined_tokens = BTreeSet::new();

    for section_name in [
        "Brand Essence",
        "Audience and Positioning",
        "Verbal Identity",
        "Logo System",
        "Color Tokens",
        "Typography",
        "Imagery",
        "Iconography",
        "Motion",
        "Composition Principles",
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
    let mut files = Vec::new();
    for folder in ["brands", "visuals"] {
        let candidate_dir = Path::new(specifications_dir).join(folder);
        if candidate_dir.exists() {
            collect_markdown_files_recursive(&candidate_dir, &mut files)?;
        }
    }

    if files.is_empty() {
        return Ok(BTreeSet::new());
    }

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
        collect_brand_token_references, is_brand_draft_path, is_brand_spec_path,
        missing_required_brand_spec_parts, unresolved_brand_token_references,
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

## Brand Essence
### Mission
- Deliver clear, usable digital experiences.

### Vision
- Be recognized for calm, accessible product expression.

### Values
- Clarity over novelty.

## Audience and Positioning
### Audience
- Teams that value readability and low-friction workflows.

### Positioning
- Reliable, modern, and approachable.

## Verbal Identity
### Personality Attributes
- Calm
- Direct

### Tone Guidelines
- Prefer concise, literal wording over decorative copy.

### Messaging Do/Don't
- Do: Explain the interface plainly.
- Don't: Use hype language.

## Logo System
### Mark Description
- Primary mark: Circular monogram built from the letterform `T`.

### Clear Space and Sizing
- `brand.logo.clear_space.default`: `16px`

### Usage Rules
- Use the icon alone only in compact contexts.

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

## Imagery
### Style Attributes
- Clean
- Natural

### Subject Guidance
- Use scenes that reinforce clarity and focus.

### Avoid
- Avoid decorative or overly abstract imagery.

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

## Composition Principles
### Hierarchy
- Prioritize strong contrast and obvious reading order.

### Density
- Prefer generous whitespace over dense packing.

### Emphasis
- Use accent color sparingly for focal points.

## Layout Principles
- Prefer generous whitespace over dense packing.

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
    fn accepts_layout_principles_as_body_only_section() {
        let spec = valid_brand_spec();
        validate_brand_spec_content(spec).expect("layout principles should be accepted");
    }

    #[test]
    fn missing_required_brand_parts_reports_multiple_missing_items() {
        let spec = r#"# Wrong Title

## Description
Ok.

## Color Tokens
### Primary
- `brand.colors.primary.default`: `#112233`

## Typography
### Families
- `brand.typography.families.primary`: `Inter`

## Iconography
### Style
- `brand.iconography.style.default`: `outlined`

## Motion
### Durations
- `brand.motion.durations.fast`: `150ms`

## Token Reference Rules
- Use dotted tokens."#;

        let missing = missing_required_brand_spec_parts(spec);
        assert!(missing
            .iter()
            .any(|m| m.contains("Missing title '# Brand Identity Specification'")));
        assert!(missing
            .iter()
            .any(|m| m == "Missing section '## Brand Metadata'"));
        assert!(missing
            .iter()
            .any(|m| m == "Missing subsection '### Secondary' under '## Color Tokens'"));
        assert!(missing
            .iter()
            .any(|m| m == "Missing subsection '### Scales' under '## Typography'"));
        assert!(missing
            .iter()
            .any(|m| m == "Missing subsection '### Size Set' under '## Iconography'"));
        assert!(missing
            .iter()
            .any(|m| m == "Missing subsection '### Easing' under '## Motion'"));
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
    fn accepts_identity_first_optional_sections_in_canonical_order() {
        let validation = validate_brand_spec_content(valid_brand_spec()).expect("valid brand spec");
        assert!(validation
            .defined_tokens
            .contains("brand.logo.clear_space.default"));
    }

    #[test]
    fn collects_references_from_markdown() {
        let refs =
            collect_brand_token_references("Use `brand.colors.primary.default` in the layout.");
        assert_eq!(refs, vec!["brand.colors.primary.default".to_string()]);
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

    #[test]
    fn brand_path_detection_accepts_visuals_and_brands() {
        assert!(is_brand_draft_path(
            std::path::Path::new("drafts/brands/acme.md"),
            "drafts"
        ));
        assert!(is_brand_draft_path(
            std::path::Path::new("drafts/visuals/snake.md"),
            "drafts"
        ));
        assert!(is_brand_spec_path(
            std::path::Path::new("specifications/brands/acme.md"),
            "specifications"
        ));
        assert!(is_brand_spec_path(
            std::path::Path::new("specifications/visuals/snake.md"),
            "specifications"
        ));
    }

    #[test]
    fn unresolved_reference_detection_reads_visuals_and_brands() {
        let root = temp_root("visuals_refs");
        let specs = root.join("specifications");
        fs::create_dir_all(specs.join("visuals")).expect("mkdir visuals");
        fs::write(specs.join("visuals/snake.md"), valid_brand_spec()).expect("write visuals spec");

        let unresolved = unresolved_brand_token_references(
            "brand.colors.primary.default brand.layout.breakpoints.xl",
            specs.to_str().expect("spec path"),
        )
        .expect("resolved refs");

        assert_eq!(unresolved, vec!["brand.layout.breakpoints.xl".to_string()]);

        let _ = fs::remove_dir_all(root);
    }
}
