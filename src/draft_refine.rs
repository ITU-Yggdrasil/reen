use crate::draft_parser::{
    BrandDraft, ComponentDraft, ContextDraft, DataDraft, DraftDocument, ProjectionDraft,
    RoutingTableDraft, parse_draft_file,
};
use crate::workspace::Workspace;
use anyhow::{Context, Result};
use regex::Regex;
use std::fs;
use std::path::PathBuf;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DraftReviewCategory {
    Completeness,
    Concreteness,
    Ambiguity,
    UnresolvedReference,
}

impl DraftReviewCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Completeness => "completeness",
            Self::Concreteness => "concreteness",
            Self::Ambiguity => "ambiguity",
            Self::UnresolvedReference => "unresolved_reference",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftReviewFinding {
    pub draft_path: String,
    pub source_line: Option<usize>,
    pub category: DraftReviewCategory,
    pub message: String,
    pub severity: Option<u8>,
}

#[derive(Debug, Clone, Default)]
pub struct DraftReview {
    pub reviewed_paths: Vec<String>,
    pub findings: Vec<DraftReviewFinding>,
}

const VAGUE_PHRASES: &[&str] = &[
    "appropriately",
    "if needed",
    "when possible",
    "as required",
    "properly",
    "correctly",
    "suitable",
    "relevant",
    "the right thing",
    "the right way",
    "the right approach",
    "current situation",
    "etc.",
];

pub fn review_raw_drafts(workspace: &Workspace, paths: &[PathBuf]) -> Result<DraftReview> {
    let mut report = DraftReview {
        reviewed_paths: paths
            .iter()
            .map(|p| {
                p.strip_prefix(&workspace.root)
                    .unwrap_or(p)
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect(),
        findings: Vec::new(),
    };

    for path in paths {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let relative = path
            .strip_prefix(&workspace.root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");

        // Code-token lint (runs on raw content, before parsing)
        // Exempt generated files under drafts/.apis/ — they contain code-level tokens by design
        let is_generated_api = relative.contains("drafts/.apis/") || relative.contains(".apis/");
        if !is_generated_api {
            let code_token_findings = scan_code_tokens(&content, &relative);
            report.findings.extend(code_token_findings);
        }

        // Web-kind specific lint (stricter rules for component/brand/routing-table)
        let rel_path = std::path::Path::new(&relative);
        if let Some(kind) = crate::draft_parser::ArtifactKind::from_draft_path(rel_path) {
            let web_findings = scan_web_kind_tokens(&content, &relative, kind);
            report.findings.extend(web_findings);
        }

        match parse_draft_file(path, &workspace.drafts_dir, &content) {
            Ok(document) => {
                review_document(&relative, &content, &document, &mut report.findings);
            }
            Err(error) => {
                report.findings.push(DraftReviewFinding {
                    draft_path: relative,
                    source_line: None,
                    category: DraftReviewCategory::Completeness,
                    message: error.to_string(),
                    severity: None,
                });
            }
        }
    }

    sort_and_dedup_findings(&mut report.findings);
    Ok(report)
}

pub fn sort_and_dedup_findings(findings: &mut Vec<DraftReviewFinding>) {
    findings.sort_by(|a, b| {
        (
            a.draft_path.as_str(),
            a.source_line.unwrap_or(usize::MAX),
            a.category,
            a.message.as_str(),
        )
            .cmp(&(
                b.draft_path.as_str(),
                b.source_line.unwrap_or(usize::MAX),
                b.category,
                b.message.as_str(),
            ))
    });
    findings.dedup_by(|a, b| {
        a.draft_path == b.draft_path
            && a.source_line == b.source_line
            && a.category == b.category
            && a.message == b.message
    });
}

fn review_document(
    path: &str,
    content: &str,
    doc: &DraftDocument,
    findings: &mut Vec<DraftReviewFinding>,
) {
    match doc {
        DraftDocument::Data(data) => review_data(path, content, data, findings),
        DraftDocument::Projection(proj) => review_projection(path, content, proj, findings),
        DraftDocument::Context(ctx) => review_context(path, content, ctx, findings),
        DraftDocument::Component(comp) => review_component(path, content, comp, findings),
        DraftDocument::Brand(brand) => review_brand(path, content, brand, findings),
        DraftDocument::RoutingTable(rt) => review_routing_table(path, content, rt, findings),
        DraftDocument::App(_) => {} // App drafts have no additional review rules
    }
}

fn review_data(
    path: &str,
    content: &str,
    data: &DataDraft,
    findings: &mut Vec<DraftReviewFinding>,
) {
    if data.attributes.is_empty() {
        push(findings, path, heading_line(content, "## Attributes"),
            DraftReviewCategory::Completeness,
            "data draft must declare at least one attribute");
    }
    if !data.operations.is_empty() && data.scenarios.is_empty() {
        push(findings, path, heading_line(content, "## Scenarios"),
            DraftReviewCategory::Completeness,
            "data draft with operations must contain at least one scenario");
    }
    scan_vagueness(findings, path, content, &data.purpose, "purpose");
    for op in &data.operations {
        scan_vagueness(findings, path, content, &op.description, &format!("operation `{}`", op.name));
    }
}

fn review_projection(
    path: &str,
    content: &str,
    proj: &ProjectionDraft,
    findings: &mut Vec<DraftReviewFinding>,
) {
    if proj.source.is_empty() {
        push(findings, path, heading_line(content, "## Source"),
            DraftReviewCategory::Completeness,
            "projection draft must declare a source");
    }
    if proj.queries.is_empty() {
        push(findings, path, heading_line(content, "## Queries"),
            DraftReviewCategory::Completeness,
            "projection draft must declare at least one query");
    }
    if !proj.queries.is_empty() && proj.scenarios.is_empty() {
        push(findings, path, heading_line(content, "## Scenarios"),
            DraftReviewCategory::Completeness,
            "projection draft with queries must contain at least one scenario");
    }
    scan_vagueness(findings, path, content, &proj.purpose, "purpose");
}

fn review_context(
    path: &str,
    content: &str,
    ctx: &ContextDraft,
    findings: &mut Vec<DraftReviewFinding>,
) {
    if ctx.role_players.is_empty() {
        push(findings, path, heading_line(content, "## Role Players"),
            DraftReviewCategory::Completeness,
            "context draft must declare at least one role player");
    }
    if ctx.functionalities.is_empty() {
        push(findings, path, heading_line(content, "## Functionalities"),
            DraftReviewCategory::Completeness,
            "context draft must declare at least one functionality");
    }
    if ctx.scenarios.is_empty() {
        push(findings, path, heading_line(content, "## Scenarios"),
            DraftReviewCategory::Completeness,
            "context draft must contain at least one scenario");
    }
    // Every role player must have role methods. A role player without
    // methods is a prop, not a role — demote it to the Props section.
    for rp in &ctx.role_players {
        let has_group = ctx.role_methods.iter().any(|g| g.role == rp.name);
        if !has_group {
            push(findings, path, find_line(content, &rp.name),
                DraftReviewCategory::Completeness,
                &format!(
                    "role player `{}` has no role methods — a role is extended with \
                     role methods inside a context; if `{}` is used as-is without \
                     extension, declare it under ## Props instead",
                    rp.name, rp.name
                ));
        }
    }
    scan_vagueness(findings, path, content, &ctx.purpose, "purpose");
    for func in &ctx.functionalities {
        scan_vagueness(findings, path, content, &func.description, &format!("functionality `{}`", func.name));
    }
}

fn review_component(
    path: &str,
    content: &str,
    comp: &ComponentDraft,
    findings: &mut Vec<DraftReviewFinding>,
) {
    let has_behaviour = !comp.events.is_empty() || comp.composition.is_some();
    if has_behaviour && comp.scenarios.is_empty() {
        push(findings, path, heading_line(content, "## Scenarios"),
            DraftReviewCategory::Completeness,
            "component draft with events or composition must contain at least one scenario");
    }
    scan_vagueness(findings, path, content, &comp.purpose, "purpose");
}

fn review_brand(
    path: &str,
    content: &str,
    brand: &BrandDraft,
    findings: &mut Vec<DraftReviewFinding>,
) {
    if brand.token_groups.is_empty() && brand.voice.is_none() && brand.prose_sections.is_empty() {
        push(findings, path, None,
            DraftReviewCategory::Completeness,
            "brand draft must declare at least one section (Voice, Visual Identity, Motion, etc.)");
    }
    scan_vagueness(findings, path, content, &brand.purpose, "purpose");
}

fn review_routing_table(
    path: &str,
    content: &str,
    rt: &RoutingTableDraft,
    findings: &mut Vec<DraftReviewFinding>,
) {
    if rt.routes.is_empty() {
        push(findings, path, heading_line(content, "## Routes"),
            DraftReviewCategory::Completeness,
            "routing table draft must declare at least one route");
    }
    scan_vagueness(findings, path, content, &rt.purpose, "purpose");
}

fn scan_vagueness(
    findings: &mut Vec<DraftReviewFinding>,
    path: &str,
    content: &str,
    text: &str,
    context: &str,
) {
    let lower = text.to_ascii_lowercase();
    for phrase in VAGUE_PHRASES {
        if lower.contains(phrase) {
            push(findings, path, find_line(content, phrase),
                DraftReviewCategory::Concreteness,
                &format!("{context} uses vague phrase `{phrase}` without an explicit criterion"));
        }
    }
}

fn push(
    findings: &mut Vec<DraftReviewFinding>,
    path: &str,
    line: Option<usize>,
    category: DraftReviewCategory,
    message: &str,
) {
    findings.push(DraftReviewFinding {
        draft_path: path.to_string(),
        source_line: line,
        category,
        message: message.to_string(),
        severity: None,
    });
}

fn heading_line(content: &str, heading: &str) -> Option<usize> {
    find_line(content, heading)
}

fn find_line(content: &str, needle: &str) -> Option<usize> {
    if needle.trim().is_empty() {
        return None;
    }
    let needle = needle.trim().to_ascii_lowercase();
    content.lines().enumerate().find_map(|(idx, line)| {
        if line.to_ascii_lowercase().contains(&needle) {
            Some(idx + 1)
        } else {
            None
        }
    })
}

// ── Code-token lint ──────────────────────────────────────────────

struct CodeTokenPattern {
    regex: Regex,
    family: &'static str,
}

static CODE_TOKEN_PATTERNS: LazyLock<Vec<CodeTokenPattern>> = LazyLock::new(|| {
    let patterns: Vec<(&str, &str)> = vec![
        // Family 1: Signature lines
        (r"(?i)^\s*signature\s*:", "signature line"),
        (r"`fn\s+\w+", "backtick-fenced fn declaration"),
        // Family 2: Rust path syntax
        (r"\w+::\w+", "Rust path syntax (::)"),
        // Family 3: Generic-type brackets
        (r"\b(Vec|Option|Result|HashMap|BTreeMap|HashSet|BTreeSet)\s*<", "generic type bracket"),
        (r"&\s*\[", "slice reference"),
        // Family 4: Reference/mutability sigils
        (r"&self\b", "reference sigil (&self)"),
        (r"&\s*mut\s", "mutable reference sigil (&mut)"),
        (r"\b(Box|Arc|Rc|RefCell|Cow)\s*<", "smart pointer generic"),
        // Family 5: Turbofish
        (r"::<", "turbofish syntax"),
        // Family 6: Trait-bound syntax
        (r"\bimpl\s+[A-Z]", "impl Trait syntax"),
        (r"\bdyn\s+[A-Z]", "dyn Trait syntax"),
        (r"\bwhere\s+\w+\s*:", "where clause"),
        // Family 7: Primitive type tokens
        (r"\b(u8|u16|u32|u64|u128|i8|i16|i32|i64|i128|f32|f64|usize|isize|bool|char)\b", "Rust primitive type"),
        (r"\bstr\b", "Rust primitive type (str)"),
        (r"\bString\b", "Rust type (String)"),
        (r"\d_?(u8|u16|u32|u64|u128|i8|i16|i32|i64|i128|f32|f64|usize|isize)\b", "numeric literal with type suffix"),
        (r"\bas\s+(u8|u16|u32|u64|u128|i8|i16|i32|i64|i128|f32|f64|usize|isize)\b", "Rust cast (as type)"),
        // Family 7 extra: return-type arrow
        (r"\s->\s", "return-type arrow (->)"),
        // Family 8: Method-call syntax
        (r"\.\w+\(", "method-call syntax"),
        // Family 9: Macro invocations
        (r"\w+!\s*[\(\[\{]", "macro invocation"),
    ];
    patterns
        .into_iter()
        .map(|(pat, family)| CodeTokenPattern {
            regex: Regex::new(pat).expect("invalid code-token regex"),
            family,
        })
        .collect()
});

// ── Web-kind lint patterns ──────────────────────────────────────

struct WebLintPattern {
    regex: Regex,
    family: &'static str,
}

/// Framework-specific hook names rejected from component drafts.
const FRAMEWORK_HOOKS: &[&str] = &[
    "useState", "useEffect", "useContext", "useReducer", "useMemo", "useCallback",
    "useRef", "useLayoutEffect", "useImperativeHandle",
    "createSignal", "createEffect", "createMemo", "createResource",
    "use_state", "use_signal", "use_effect", "use_memo", "use_context",
    "onMount", "onCleanup", "onError",
];

static WEB_COMPONENT_PATTERNS: LazyLock<Vec<WebLintPattern>> = LazyLock::new(|| {
    let mut patterns: Vec<(&str, &str)> = vec![
        // HTML tag syntax: <div>, <input />, </span>
        (r"</?(?:div|span|input|button|a|img|form|table|tr|td|th|thead|tbody|ul|ol|li|p|section|article|nav|header|footer|main|aside|textarea|select|option|label|fieldset|legend|canvas|svg|video|audio|iframe)[\s/>]", "HTML tag"),
        // Style/class attributes
        (r#"\b(?:style|class|className|classList)\s*="#, "style/class attribute"),
        // CSS property syntax: "color: #fff", "margin: 10px", "display: flex"
        (r"\b(?:color|background|margin|padding|border|display|position|font-size|font-family|width|height|flex|grid|opacity|z-index|overflow|transform)\s*:", "CSS property syntax"),
        // Tailwind/utility class names: "bg-blue-500", "text-lg", "flex-col", "p-4", "mt-2"
        (r"\b(?:bg|text|flex|grid|p|m|mt|mb|ml|mr|mx|my|pt|pb|pl|pr|px|py|w|h|min-w|max-w|min-h|max-h|rounded|shadow|border|ring|gap|space)-(?:\d+|xs|sm|md|lg|xl|2xl|3xl|full|auto|none)\b", "Tailwind/utility class name"),
        // JSX/template syntax: {expression}, {{expression}}, v-if, :prop, @click
        (r"\{\{[^}]+\}\}", "template expression syntax"),
        // Raw hex colours: #fff, #3355ff, #rgb
        (r"#[0-9a-fA-F]{3,8}\b", "raw hex colour value"),
        // RGB/HSL colour values
        (r"\b(?:rgb|rgba|hsl|hsla)\s*\(", "rgb/hsl colour value"),
    ];

    // Framework hooks
    for hook in FRAMEWORK_HOOKS {
        patterns.push((
            // Match as whole word
            &*Box::leak(format!(r"\b{}\b", regex::escape(hook)).into_boxed_str()),
            "framework-specific hook name",
        ));
    }

    patterns
        .into_iter()
        .map(|(pat, family)| WebLintPattern {
            regex: Regex::new(pat).expect("invalid web-lint regex"),
            family,
        })
        .collect()
});

static WEB_BRAND_PATTERNS: LazyLock<Vec<WebLintPattern>> = LazyLock::new(|| {
    let patterns: Vec<(&str, &str)> = vec![
        // Hex colours
        (r"#[0-9a-fA-F]{3,8}\b", "raw hex colour value"),
        // RGB/HSL
        (r"\b(?:rgb|rgba|hsl|hsla)\s*\(", "rgb/hsl colour value"),
        // Font stacks: "Helvetica Neue, Arial, sans-serif"
        (r#"(?:Helvetica|Arial|Times|Georgia|Verdana|Courier|Roboto|Inter|Lato|Open\s+Sans|Segoe\s+UI|system-ui)\s*(?:,|Neue)"#, "specific font stack"),
        // Measurement units: 14px, 1.5rem, 16em, 100vh, 50vw, 12pt
        (r"\b\d+(?:\.\d+)?(?:px|em|rem|vh|vw|pt)\b", "measurement value (use role-based tokens instead)"),
    ];
    patterns
        .into_iter()
        .map(|(pat, family)| WebLintPattern {
            regex: Regex::new(pat).expect("invalid brand-lint regex"),
            family,
        })
        .collect()
});

static WEB_ROUTING_PATTERNS: LazyLock<Vec<WebLintPattern>> = LazyLock::new(|| {
    vec![
        WebLintPattern {
            regex: Regex::new(r"\b(?:get|post|put|delete|patch)!\s*[\(\[]").unwrap(),
            family: "framework-specific route macro",
        },
        WebLintPattern {
            regex: Regex::new(r"Regex\s*\(").unwrap(),
            family: "regex route pattern",
        },
        WebLintPattern {
            regex: Regex::new(r#"r#?"[^"]+""#).unwrap(),
            family: "raw string regex literal",
        },
        WebLintPattern {
            regex: Regex::new(r"\b[a-z_][a-z0-9_]*\s*\(").unwrap(),
            family: "function identifier as handler (use component names)",
        },
    ]
});

/// Scan web-kind draft content for prohibited patterns.
///
/// Returns findings for component, brand, or routing-table specific violations.
/// These are always blocking — no `--allow-code-tokens` escape hatch for web kinds.
pub fn scan_web_kind_tokens(
    content: &str,
    path: &str,
    kind: crate::draft_parser::ArtifactKind,
) -> Vec<DraftReviewFinding> {
    let patterns = match kind {
        crate::draft_parser::ArtifactKind::Component => &*WEB_COMPONENT_PATTERNS,
        crate::draft_parser::ArtifactKind::Brand => &*WEB_BRAND_PATTERNS,
        crate::draft_parser::ArtifactKind::RoutingTable => &*WEB_ROUTING_PATTERNS,
        _ => return Vec::new(),
    };

    let mut findings = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        let line_no = idx + 1;
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            continue;
        }
        for pattern in patterns {
            if let Some(m) = pattern.regex.find(line) {
                let matched = m.as_str();
                let snippet = context_snippet(line, m.start(), m.end());
                findings.push(DraftReviewFinding {
                    draft_path: path.to_string(),
                    source_line: Some(line_no),
                    category: DraftReviewCategory::Concreteness,
                    message: format!(
                        "web-kind lint ({family}): `{matched}` in \"{snippet}\"",
                        family = pattern.family,
                    ),
                    severity: None,
                });
            }
        }
    }
    sort_and_dedup_findings(&mut findings);
    findings
}

/// Scan draft content for code-level tokens that violate the BA-authorable bar.
///
/// Returns findings with `category: Concreteness` for every line that contains
/// a code token from one of the nine rejected families.
pub fn scan_code_tokens(content: &str, path: &str) -> Vec<DraftReviewFinding> {
    let mut findings = Vec::new();
    for (idx, line) in content.lines().enumerate() {
        let line_no = idx + 1;
        // Skip markdown headers — they are structural, not prose
        let trimmed = line.trim_start();
        if trimmed.starts_with('#') {
            continue;
        }
        for pattern in CODE_TOKEN_PATTERNS.iter() {
            if let Some(m) = pattern.regex.find(line) {
                let matched = m.as_str();
                // Build a context snippet: up to 40 chars around the match
                let snippet = context_snippet(line, m.start(), m.end());
                findings.push(DraftReviewFinding {
                    draft_path: path.to_string(),
                    source_line: Some(line_no),
                    category: DraftReviewCategory::Concreteness,
                    message: format!(
                        "code token ({family}): `{matched}` in \"{snippet}\"",
                        family = pattern.family,
                    ),
                    severity: None,
                });
            }
        }
    }
    sort_and_dedup_findings(&mut findings);
    findings
}

/// Check whether any findings are code-token concreteness issues.
pub fn has_code_token_findings(findings: &[DraftReviewFinding]) -> bool {
    findings.iter().any(|f| {
        f.category == DraftReviewCategory::Concreteness && f.message.starts_with("code token")
    })
}

/// Extract a short context snippet around a match for error messages.
fn context_snippet(line: &str, start: usize, end: usize) -> String {
    let chars: Vec<char> = line.chars().collect();
    let char_len = chars.len();

    // Convert byte offsets to char offsets
    let mut char_start = 0;
    let mut byte_count = 0;
    for (i, ch) in chars.iter().enumerate() {
        if byte_count >= start {
            char_start = i;
            break;
        }
        byte_count += ch.len_utf8();
    }

    let context_pad = 20;
    let snip_start = char_start.saturating_sub(context_pad);
    // Find the char offset for the match end
    let mut char_end = char_start;
    byte_count = start;
    for (i, ch) in chars.iter().enumerate().skip(char_start) {
        if byte_count >= end {
            char_end = i;
            break;
        }
        byte_count += ch.len_utf8();
        char_end = i + 1;
    }
    let snip_end = (char_end + context_pad).min(char_len);

    let snippet: String = chars[snip_start..snip_end].iter().collect();
    let snippet = snippet.trim();
    if snip_start > 0 || snip_end < char_len {
        format!("...{}...", snippet)
    } else {
        snippet.to_string()
    }
}

// ── Unresolved reference check (vocabulary-aware) ────────────────

/// Check all type references in a parsed draft against the vocabulary.
/// Produces findings for each unresolved reference with heuristic suggestions.
pub fn check_unresolved_references(
    path: &str,
    doc: &DraftDocument,
    vocab: &crate::manifest::DciVocabulary,
    findings: &mut Vec<DraftReviewFinding>,
) {
    match doc {
        DraftDocument::Context(ctx) => {
            for rp in &ctx.role_players {
                if !crate::prepare::is_builtin_primitive(&rp.data_type)
                    && !crate::manifest::DciVocabulary::is_api_reference(&rp.data_type)
                    && vocab.resolve(&rp.data_type).is_none()
                {
                    let (_cat, suggestion) = crate::prepare::classify_unresolved_reference(&rp.data_type);
                    findings.push(DraftReviewFinding {
                        draft_path: path.to_string(),
                        source_line: None,
                        category: DraftReviewCategory::UnresolvedReference,
                        message: format!(
                            "role player `{}` references `{}` which is not declared. {}",
                            rp.name, rp.data_type, suggestion
                        ),
                        severity: None,
                    });
                }
            }
        }
        DraftDocument::Projection(proj) => {
            if !proj.source.is_empty()
                && !crate::prepare::is_builtin_primitive(&proj.source)
                && vocab.resolve(&proj.source).is_none()
            {
                let (_cat, suggestion) = crate::prepare::classify_unresolved_reference(&proj.source);
                findings.push(DraftReviewFinding {
                    draft_path: path.to_string(),
                    source_line: None,
                    category: DraftReviewCategory::UnresolvedReference,
                    message: format!(
                        "projection source `{}` is not declared. {}",
                        proj.source, suggestion
                    ),
                    severity: None,
                });
            }
        }
        DraftDocument::Component(comp) => {
            if let Some(composition) = &comp.composition {
                for cb in &composition.child_bindings {
                    if vocab.resolve(&cb.child).is_none() {
                        let (_cat, suggestion) = crate::prepare::classify_unresolved_reference(&cb.child);
                        findings.push(DraftReviewFinding {
                            draft_path: path.to_string(),
                            source_line: None,
                            category: DraftReviewCategory::UnresolvedReference,
                            message: format!(
                                "child binding `{}` is not declared. {}",
                                cb.child, suggestion
                            ),
                            severity: None,
                        });
                    }
                }
                for eb in &composition.event_bindings {
                    if vocab.resolve(&eb.context).is_none() {
                        let (_cat, suggestion) = crate::prepare::classify_unresolved_reference(&eb.context);
                        findings.push(DraftReviewFinding {
                            draft_path: path.to_string(),
                            source_line: None,
                            category: DraftReviewCategory::UnresolvedReference,
                            message: format!(
                                "event binding context `{}` is not declared. {}",
                                eb.context, suggestion
                            ),
                            severity: None,
                        });
                    }
                }
            }
        }
        DraftDocument::RoutingTable(rt) => {
            for route in &rt.routes {
                if vocab.resolve(&route.component).is_none() {
                    let (_cat, suggestion) = crate::prepare::classify_unresolved_reference(&route.component);
                    findings.push(DraftReviewFinding {
                        draft_path: path.to_string(),
                        source_line: None,
                        category: DraftReviewCategory::UnresolvedReference,
                        message: format!(
                            "route `{}` references component `{}` which is not declared. {}",
                            route.path, route.component, suggestion
                        ),
                        severity: None,
                    });
                }
                for param in &route.params {
                    if !crate::prepare::is_builtin_primitive(&param.data_type)
                        && vocab.resolve(&param.data_type).is_none()
                    {
                        let (_cat, suggestion) = crate::prepare::classify_unresolved_reference(&param.data_type);
                        findings.push(DraftReviewFinding {
                            draft_path: path.to_string(),
                            source_line: None,
                            category: DraftReviewCategory::UnresolvedReference,
                            message: format!(
                                "route `{}` param `{}` references `{}` which is not declared. {}",
                                route.path, param.name, param.data_type, suggestion
                            ),
                            severity: None,
                        });
                    }
                }
            }
        }
        DraftDocument::App(app) => {
            if let Some(ctx_name) = &app.entry_context {
                if vocab.resolve(ctx_name).is_none() {
                    let (_cat, suggestion) = crate::prepare::classify_unresolved_reference(ctx_name);
                    findings.push(DraftReviewFinding {
                        draft_path: path.to_string(),
                        source_line: None,
                        category: DraftReviewCategory::UnresolvedReference,
                        message: format!(
                            "entry context `{}` is not declared. {}",
                            ctx_name, suggestion
                        ),
                        severity: None,
                    });
                }
            }
        }
        DraftDocument::Data(_) | DraftDocument::Brand(_) => {
            // Data and brand drafts don't have type references to other artefacts
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: scan a fragment and return whether any code-token findings were produced.
    fn flags(content: &str) -> bool {
        !scan_code_tokens(content, "test.md").is_empty()
    }

    /// Helper: scan a fragment and return all code-token finding messages.
    fn finding_messages(content: &str) -> Vec<String> {
        scan_code_tokens(content, "test.md")
            .into_iter()
            .map(|f| f.message)
            .collect()
    }

    // ── Known-bad: snake game_loop.md fragment (must flag) ───────

    #[test]
    fn flags_snake_game_loop_signature_lines() {
        let fragment = r#"
- **body**
  Signature: `body(&self) -> &Vec<Position>`
  Returns the snake positions in order, with the head first.
"#;
        let msgs = finding_messages(fragment);
        assert!(!msgs.is_empty(), "expected code-token findings");
        assert!(
            msgs.iter().any(|m| m.contains("signature line")),
            "expected Signature: finding, got: {msgs:?}"
        );
    }

    #[test]
    fn flags_snake_game_loop_path_syntax() {
        let fragment = r#"
10. If the next head lands on a wall cell, return `PlayerState::Dead`.
9. Direction::Up must decrease y by 1, Direction::Down must increase y by 1.
"#;
        let msgs = finding_messages(fragment);
        assert!(
            msgs.iter().any(|m| m.contains("Rust path syntax")),
            "expected :: path finding, got: {msgs:?}"
        );
    }

    #[test]
    fn flags_snake_game_loop_saturating_sub() {
        let fragment =
            "2. Let `elapsed_ms` be `now_ms.saturating_sub(game_state.game_started_ms())`.\n";
        let msgs = finding_messages(fragment);
        assert!(
            msgs.iter().any(|m| m.contains("method-call syntax")),
            "expected method-call finding, got: {msgs:?}"
        );
    }

    #[test]
    fn flags_snake_game_loop_as_cast_and_suffix() {
        let fragment =
            "4. Let `wait_ms` be `max(10_u64, (100.0_f64 / (1.0_f64 + elapsed_s as f64).log(10)) as u64)`.\n";
        let msgs = finding_messages(fragment);
        assert!(
            msgs.iter().any(|m| m.contains("Rust primitive type") || m.contains("numeric literal") || m.contains("Rust cast")),
            "expected primitive/cast/suffix finding, got: {msgs:?}"
        );
    }

    #[test]
    fn flags_snake_terminal_renderer_std_path() {
        let fragment = "| screen | Visible terminal area | Type `std::io::Stdout` |\n";
        let msgs = finding_messages(fragment);
        assert!(
            msgs.iter().any(|m| m.contains("Rust path syntax")),
            "expected :: path finding for std::io::Stdout, got: {msgs:?}"
        );
    }

    #[test]
    fn flags_generic_vec_option() {
        let fragment = "Signature: `body(&self) -> &Vec<Position>`\n";
        let msgs = finding_messages(fragment);
        assert!(
            msgs.iter().any(|m| m.contains("generic type bracket")),
            "expected Vec< finding, got: {msgs:?}"
        );
    }

    #[test]
    fn flags_option_food() {
        let fragment = "Signature: `food(&self) -> Option<Food>`\n";
        let msgs = finding_messages(fragment);
        assert!(
            msgs.iter().any(|m| m.contains("generic type bracket")),
            "expected Option< finding, got: {msgs:?}"
        );
    }

    #[test]
    fn flags_macro_invocation() {
        let fragment = "Use println!(\"hello\") to show the result.\n";
        let msgs = finding_messages(fragment);
        assert!(
            msgs.iter().any(|m| m.contains("macro invocation")),
            "expected macro finding, got: {msgs:?}"
        );
    }

    #[test]
    fn flags_return_arrow() {
        let fragment = "Signature: `get_score(&self) -> u32`\n";
        let msgs = finding_messages(fragment);
        assert!(
            msgs.iter().any(|m| m.contains("return-type arrow")),
            "expected -> finding, got: {msgs:?}"
        );
    }

    #[test]
    fn flags_self_reference() {
        let fragment = "Signature: `body(&self) -> &Vec<Position>`\n";
        let msgs = finding_messages(fragment);
        assert!(
            msgs.iter().any(|m| m.contains("&self")),
            "expected &self finding, got: {msgs:?}"
        );
    }

    // ── Known-good: money-transfer fragment (must pass) ─────────

    #[test]
    fn passes_money_transfer_context() {
        let fragment = r#"
## Purpose

MoneyTransfer enacts the transfer of a specific amount from one account to
another. It coordinates the source and sink accounts through the ledger,
ensuring that the source has sufficient funds and that the resulting ledger
entries correctly represent the movement of money.

## Role Players

- source: Account — provides the funds; must have sufficient balance to cover the transfer
- sink: Account — receives the funds

## Props

- amount: the Amount to transfer between the accounts
- ledger: the Ledger that records all transactions

## Role Methods

### source
- withdraw: creates an unsettled ledger entry representing a withdrawal of the
  transfer amount from the source account; the transfer amount must not exceed
  the source's current balance; returns an error if business rules are violated

### sink
- deposit: settles the unsettled entry created by withdraw, recording the sink
  as the destination; the resulting entry represents a complete transfer from
  source to sink

## Functionalities

### new
Constructs the money transfer context. Receives the account identifiers for
source and sink, the amount, and the ledger. Constructs Account views for both
source and sink. The currency of the amount must match the currency of the
source account. Returns an error if account construction fails or if the
currency does not match.

### transfer
Executes the transfer by calling the source's withdraw role method, then the
sink's deposit role method, then committing the resulting entry to the ledger.
If any step fails, the operation returns an error immediately. On success,
returns the updated ledger.

## Scenarios

### successful transfer
Given: account 123456 has balance 1000.00 DKK, account 654321 has balance 1000.00 DKK, amount is 250.00 DKK
When: transfer is called
Then: the returned ledger shows account 123456 with balance 750.00 DKK and account 654321 with balance 1250.00 DKK

### insufficient funds
Given: account 123456 has balance 100.00 DKK, amount is 250.00 DKK
When: transfer is called
Then: an error is returned indicating insufficient funds

### currency mismatch on construction
Given: account 123456 uses DKK, amount is 250.00 EUR
When: new is called
Then: an error is returned indicating the currency does not match
"#;
        let findings = scan_code_tokens(fragment, "drafts/contexts/money_transfer.md");
        assert!(
            findings.is_empty(),
            "expected no code-token findings in clean money-transfer draft, got:\n{}",
            findings
                .iter()
                .map(|f| format!("  line {}: {}", f.source_line.unwrap_or(0), f.message))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    // ── Borderline cases ────────────────────────────────────────

    #[test]
    fn passes_decimal_amount() {
        assert!(!flags("Transfer 250.00 DKK from account 123456."));
    }

    #[test]
    fn flags_returns_u32() {
        assert!(flags("returns a u32"));
    }

    #[test]
    fn flags_amount_colon_u32() {
        assert!(flags("amount: u32"));
    }

    #[test]
    fn passes_score_is_20() {
        assert!(!flags("the score is 20"));
    }

    #[test]
    fn passes_to_str_operation_name() {
        assert!(!flags("to_str is called"));
    }

    #[test]
    fn flags_capital_string_type() {
        assert!(flags("Returns a String representation of the entry."));
    }

    #[test]
    fn passes_lowercase_string_word() {
        assert!(!flags("Returns a string representation of the entry."));
    }

    #[test]
    fn passes_english_contraction() {
        assert!(!flags("can't process that request"));
    }

    #[test]
    fn passes_business_identifiers() {
        assert!(!flags("account 123456 and 654321"));
    }

    #[test]
    fn passes_absent_for_withdrawal() {
        assert!(!flags("sink is absent for a withdrawal"));
    }

    #[test]
    fn passes_domain_vocabulary() {
        assert!(!flags("the result is an Amount of 0 in DKK"));
    }

    #[test]
    fn passes_prev_hash_in_prose() {
        assert!(!flags("prev_hash matches the head hash"));
    }

    #[test]
    fn flags_method_call_chain() {
        assert!(flags("game_state.score()"));
    }

    #[test]
    fn flags_dot_unwrap() {
        assert!(flags(".unwrap()"));
    }

    #[test]
    fn flags_as_f64_cast() {
        assert!(flags("elapsed_s as f64"));
    }

    #[test]
    fn flags_numeric_type_suffix() {
        assert!(flags("100.0_f64"));
    }

    #[test]
    fn passes_comparison_in_prose() {
        assert!(!flags("x >= width or y >= height"));
    }

    #[test]
    fn skips_markdown_headers() {
        // Headers are structural — code tokens in them should not flag
        assert!(!flags("# Context: MoneyTransfer"));
        assert!(!flags("## Role Players"));
        assert!(!flags("### source"));
    }

    // ── Web-kind lint tests ────────────────────────────────────

    fn web_flags(content: &str, kind: crate::draft_parser::ArtifactKind) -> bool {
        !scan_web_kind_tokens(content, "test.md", kind).is_empty()
    }

    fn web_messages(content: &str, kind: crate::draft_parser::ArtifactKind) -> Vec<String> {
        scan_web_kind_tokens(content, "test.md", kind)
            .into_iter()
            .map(|f| f.message)
            .collect()
    }

    // Component lint: HTML tags
    #[test]
    fn component_flags_html_div() {
        assert!(web_flags(
            "Render a <div> with the content",
            crate::draft_parser::ArtifactKind::Component
        ));
    }

    #[test]
    fn component_flags_html_input() {
        assert!(web_flags(
            "An <input type=\"email\"> for the user",
            crate::draft_parser::ArtifactKind::Component
        ));
    }

    #[test]
    fn component_passes_clean_prose() {
        // "the user types their email" should not flag
        assert!(!web_flags(
            "the user types their email into the field",
            crate::draft_parser::ArtifactKind::Component
        ));
    }

    // Component lint: CSS property
    #[test]
    fn component_flags_css_property() {
        assert!(web_flags(
            "color: #fff;",
            crate::draft_parser::ArtifactKind::Component
        ));
    }

    // Component lint: Tailwind
    #[test]
    fn component_flags_tailwind_classes() {
        assert!(web_flags(
            "Apply bg-blue-500 and text-lg",
            crate::draft_parser::ArtifactKind::Component
        ));
    }

    // Component lint: framework hooks
    #[test]
    fn component_flags_use_state() {
        assert!(web_flags(
            "Uses useState to track the value",
            crate::draft_parser::ArtifactKind::Component
        ));
    }

    #[test]
    fn component_flags_create_signal() {
        assert!(web_flags(
            "Create a createSignal for tracking",
            crate::draft_parser::ArtifactKind::Component
        ));
    }

    #[test]
    fn component_flags_use_signal() {
        assert!(web_flags(
            "Call use_signal for reactive state",
            crate::draft_parser::ArtifactKind::Component
        ));
    }

    // Component lint: hex colour
    #[test]
    fn component_flags_hex_colour() {
        assert!(web_flags(
            "Background is #3355ff",
            crate::draft_parser::ArtifactKind::Component
        ));
    }

    // Component lint: rgb colour
    #[test]
    fn component_flags_rgb_colour() {
        assert!(web_flags(
            "Uses rgb(50, 100, 200)",
            crate::draft_parser::ArtifactKind::Component
        ));
    }

    // Component lint: style attribute
    #[test]
    fn component_flags_classname() {
        assert!(web_flags(
            r#"className="header-bar""#,
            crate::draft_parser::ArtifactKind::Component
        ));
    }

    // Component lint: template syntax
    #[test]
    fn component_flags_template_expression() {
        assert!(web_flags(
            "Display {{user.name}} in the header",
            crate::draft_parser::ArtifactKind::Component
        ));
    }

    // Brand lint: hex colour
    #[test]
    fn brand_flags_hex_colour() {
        assert!(web_flags(
            "primary: #ff6600",
            crate::draft_parser::ArtifactKind::Brand
        ));
    }

    // Brand lint: font stack
    #[test]
    fn brand_flags_font_stack() {
        assert!(web_flags(
            "Heading font: Helvetica Neue, Arial",
            crate::draft_parser::ArtifactKind::Brand
        ));
    }

    // Brand lint: measurement
    #[test]
    fn brand_flags_px_value() {
        assert!(web_flags(
            "Base size: 14px",
            crate::draft_parser::ArtifactKind::Brand
        ));
    }

    #[test]
    fn brand_flags_rem_value() {
        assert!(web_flags(
            "Spacing unit: 1.5rem",
            crate::draft_parser::ArtifactKind::Brand
        ));
    }

    #[test]
    fn brand_passes_role_based_token() {
        // "background of primary CTAs" — no hex, no px
        assert!(!web_flags(
            "primary-action: background of primary CTAs",
            crate::draft_parser::ArtifactKind::Brand
        ));
    }

    // Routing table lint: framework macro
    #[test]
    fn routing_flags_get_macro() {
        assert!(web_flags(
            "get!(\"/api/users\")",
            crate::draft_parser::ArtifactKind::RoutingTable
        ));
    }

    // Routing table lint: regex
    #[test]
    fn routing_flags_regex_pattern() {
        assert!(web_flags(
            "Regex(r\"^/api/.*\")",
            crate::draft_parser::ArtifactKind::RoutingTable
        ));
    }

    #[test]
    fn routing_passes_clean_route() {
        // Headers are skipped, only prose is checked
        // A clean route section with only "path:" and "component:" should pass
        // unless function identifiers trigger
        let findings = scan_web_kind_tokens(
            "## Routes\n- path: /products\n  component: ProductScreen\n",
            "test.md",
            crate::draft_parser::ArtifactKind::RoutingTable,
        );
        // path: and component: are clean structured fields, not function calls
        // The function-identifier pattern checks for word( — "path:" doesn't match
        assert!(
            findings.is_empty(),
            "expected no findings for clean route, got: {:?}",
            findings.iter().map(|f| &f.message).collect::<Vec<_>>()
        );
    }

    // ── Unresolved reference check tests ─────────────────────────

    fn empty_vocab() -> crate::manifest::DciVocabulary {
        crate::manifest::DciVocabulary::empty()
    }

    fn vocab_with(entries: &[(&str, &str)]) -> crate::manifest::DciVocabulary {
        let mut v = crate::manifest::DciVocabulary::empty();
        for (name, kind) in entries {
            v.add(name, kind);
        }
        v
    }

    #[test]
    fn refine_flags_implementation_speak_role_player() {
        let content = "# Context: Bad\n\n## Purpose\nTest.\n\n## Role Players\n\
                       - store: SessionStore — stores sessions\n\n\
                       ## Role Methods\n### store\n- save: saves\n\n\
                       ## Functionalities\n### act\nCalls store save.\n\n\
                       ## Scenarios\n### ok\nGiven: x\nWhen: act\nThen: done\n";
        let doc = crate::draft_parser::parse_draft_file(
            std::path::Path::new("drafts/contexts/bad.md"),
            std::path::Path::new("drafts"),
            content,
        ).unwrap();
        let mut findings = Vec::new();
        check_unresolved_references("contexts/bad.md", &doc, &empty_vocab(), &mut findings);
        assert!(
            findings.iter().any(|f|
                f.category == DraftReviewCategory::UnresolvedReference
                && f.message.contains("SessionStore")
                && f.message.contains("implementation-speak")
            ),
            "should flag SessionStore as implementation-speak: {:?}",
            findings
        );
    }

    #[test]
    fn refine_passes_declared_role_player() {
        let content = "# Context: Good\n\n## Purpose\nTest.\n\n## Role Players\n\
                       - account: Account — the account\n\n\
                       ## Role Methods\n### account\n- debit: debits\n\n\
                       ## Functionalities\n### act\nCalls account debit.\n\n\
                       ## Scenarios\n### ok\nGiven: x\nWhen: act\nThen: done\n";
        let doc = crate::draft_parser::parse_draft_file(
            std::path::Path::new("drafts/contexts/good.md"),
            std::path::Path::new("drafts"),
            content,
        ).unwrap();
        let mut findings = Vec::new();
        check_unresolved_references("contexts/good.md", &doc, &vocab_with(&[("Account", "data")]), &mut findings);
        assert!(
            findings.is_empty(),
            "declared type should not produce findings: {:?}",
            findings
        );
    }

    #[test]
    fn refine_flags_unresolved_projection_source() {
        let content = "# Projection: BadView\n\n## Purpose\nTest.\n\n## Source\n- Widget\n\n\
                       ## Queries\n### summary\nReturns a summary.\n\n\
                       ## Scenarios\n### ok\nGiven: x\nWhen: summary\nThen: done\n";
        let doc = crate::draft_parser::parse_draft_file(
            std::path::Path::new("drafts/projections/bad_view.md"),
            std::path::Path::new("drafts"),
            content,
        ).unwrap();
        let mut findings = Vec::new();
        check_unresolved_references("projections/bad_view.md", &doc, &empty_vocab(), &mut findings);
        assert!(
            findings.iter().any(|f|
                f.category == DraftReviewCategory::UnresolvedReference
                && f.message.contains("Widget")
            ),
            "should flag unresolved projection source: {:?}",
            findings
        );
    }

    // ── Behaviour-based scenario rule tests ──────────────────────

    fn review_findings(content: &str, kind_dir: &str) -> Vec<DraftReviewFinding> {
        let path_str = format!("drafts/{}/test.md", kind_dir);
        let path = std::path::Path::new(&path_str);
        let doc = crate::draft_parser::parse_draft_file(
            path, std::path::Path::new("drafts"), content,
        ).unwrap();
        let relative = format!("{}/test.md", kind_dir);
        let mut findings = Vec::new();
        review_document(&relative, content, &doc, &mut findings);
        findings
    }

    #[test]
    fn data_without_operations_skips_scenario_check() {
        let content = "# Data: Amount\n\n## Purpose\nA monetary value.\n\n\
                       ## Attributes\n- value: the numeric quantity\n\n\
                       ## Invariants\n- value must not be negative\n";
        let findings = review_findings(content, "data");
        assert!(
            !findings.iter().any(|f| f.message.contains("scenario")),
            "pure-value data should not require scenarios: {:?}",
            findings.iter().map(|f| &f.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn data_with_operations_requires_scenarios() {
        let content = "# Data: Amount\n\n## Purpose\nA monetary value.\n\n\
                       ## Attributes\n- value: the numeric quantity\n\n\
                       ## Operations\n### add\nAdds two amounts.\n";
        let findings = review_findings(content, "data");
        assert!(
            findings.iter().any(|f| f.message.contains("scenario")),
            "data with operations should require scenarios: {:?}",
            findings.iter().map(|f| &f.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn pure_display_component_skips_scenario_check() {
        let content = "# Component: Button\n\n## Purpose\nA clickable button.\n\n\
                       ## Props\n- label: the text shown on the button\n";
        let findings = review_findings(content, "components");
        assert!(
            !findings.iter().any(|f| f.message.contains("scenario")),
            "pure-display component should not require scenarios: {:?}",
            findings.iter().map(|f| &f.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn component_with_events_requires_scenarios() {
        let content = "# Component: LoginForm\n\n## Purpose\nLets user log in.\n\n\
                       ## Events\n- submitted: user submitted credentials\n";
        let findings = review_findings(content, "components");
        assert!(
            findings.iter().any(|f| f.message.contains("scenario")),
            "component with events should require scenarios: {:?}",
            findings.iter().map(|f| &f.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn component_with_composition_requires_scenarios() {
        let content = "# Component: LoginScreen\n\n## Purpose\nThe login screen.\n\n\
                       ## Composition\n- LoginForm: receives credentials\n\
                       - when LoginForm.submitted: invoke AuthenticateUser\n";
        let findings = review_findings(content, "components");
        assert!(
            findings.iter().any(|f| f.message.contains("scenario")),
            "component with composition should require scenarios: {:?}",
            findings.iter().map(|f| &f.message).collect::<Vec<_>>()
        );
    }

    #[test]
    fn role_without_methods_flagged_as_prop() {
        let content = "# Context: Transfer\n\n## Purpose\nTransfers money.\n\n\
                       ## Role Players\n- source: Account — the source\n\
                       - amount: Money — the transfer amount\n\n\
                       ## Role Methods\n### source\n- debit: removes funds\n\n\
                       ## Functionalities\n### transfer\nDebits source by amount.\n\n\
                       ## Scenarios\n### ok\nGiven: x\nWhen: transfer\nThen: done\n";
        let findings = review_findings(content, "contexts");
        assert!(
            findings.iter().any(|f| f.message.contains("amount") && f.message.contains("Props instead")),
            "role without methods should redirect to Props: {:?}",
            findings.iter().map(|f| &f.message).collect::<Vec<_>>()
        );
    }
}
