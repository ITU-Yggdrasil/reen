use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct GeneratedDraftArtifact {
    pub name: String,
    pub draft_markdown: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ExternalApiExpansion {
    pub api_name: String,
    #[serde(default)]
    pub data_drafts: Vec<GeneratedDraftArtifact>,
    #[serde(default)]
    pub context_drafts: Vec<GeneratedDraftArtifact>,
}

pub fn parse_external_api_expansion(
    output: &str,
    fallback_api_name: &str,
) -> Result<ExternalApiExpansion> {
    let json_candidate = extract_json_candidate(output);
    let mut expansion: ExternalApiExpansion = serde_json::from_str(json_candidate)
        .context("external API expansion output was not valid JSON")?;

    if expansion.api_name.trim().is_empty() {
        expansion.api_name = fallback_api_name.to_string();
    }
    if expansion.context_drafts.is_empty() {
        anyhow::bail!("external API expansion did not produce any context drafts");
    }

    for artifact in expansion
        .data_drafts
        .iter_mut()
        .chain(expansion.context_drafts.iter_mut())
    {
        artifact.name = artifact.name.trim().to_string();
        artifact.draft_markdown = artifact.draft_markdown.trim().to_string();
        if artifact.name.is_empty() {
            anyhow::bail!("external API expansion returned a draft without a name");
        }
        if artifact.draft_markdown.is_empty() {
            anyhow::bail!(
                "external API expansion returned an empty draft for '{}'",
                artifact.name
            );
        }
    }

    Ok(expansion)
}

pub fn sanitize_generated_artifact_name(name: &str) -> String {
    let mut output = String::new();
    let mut last_was_separator = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            output.push(ch);
            last_was_separator = false;
        } else if !last_was_separator {
            output.push('_');
            last_was_separator = true;
        }
    }
    let cleaned = output.trim_matches('_');
    if cleaned.is_empty() {
        "generated".to_string()
    } else {
        cleaned.to_string()
    }
}

fn strip_code_fence(output: &str) -> &str {
    let trimmed = output.trim();
    if let Some(rest) = trimmed.strip_prefix("```json") {
        return rest.strip_suffix("```").unwrap_or(rest).trim();
    }
    if let Some(rest) = trimmed.strip_prefix("```") {
        return rest.strip_suffix("```").unwrap_or(rest).trim();
    }
    trimmed
}

fn extract_json_candidate(output: &str) -> &str {
    let trimmed = strip_code_fence(output);
    if trimmed.starts_with('{') {
        return trimmed;
    }

    if let Some(candidate) = extract_fenced_json_block(output) {
        return candidate;
    }

    if let Some(candidate) = extract_balanced_json_object(output) {
        return candidate;
    }

    trimmed
}

fn extract_fenced_json_block(output: &str) -> Option<&str> {
    let mut remainder = output;
    while let Some(start) = remainder.find("```") {
        let after_ticks = &remainder[start + 3..];
        let newline_idx = after_ticks.find('\n')?;
        let block_body_start = start + 3 + newline_idx + 1;
        let language = after_ticks[..newline_idx].trim();
        let rest = &remainder[block_body_start..];
        let end = rest.find("```")?;
        let body = rest[..end].trim();
        if language.is_empty() || language.eq_ignore_ascii_case("json") {
            if body.starts_with('{') {
                return Some(body);
            }
            if let Some(candidate) = extract_balanced_json_object(body) {
                return Some(candidate);
            }
        }
        remainder = &rest[end + 3..];
    }
    None
}

fn extract_balanced_json_object(output: &str) -> Option<&str> {
    let bytes = output.as_bytes();
    let mut start_idx = None;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (idx, &byte) in bytes.iter().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
                continue;
            }
            match byte {
                b'\\' => escaped = true,
                b'"' => in_string = false,
                _ => {}
            }
            continue;
        }

        match byte {
            b'"' => in_string = true,
            b'{' => {
                if depth == 0 {
                    start_idx = Some(idx);
                }
                depth += 1;
            }
            b'}' => {
                if depth == 0 {
                    continue;
                }
                depth -= 1;
                if depth == 0 {
                    if let Some(start) = start_idx {
                        return Some(output[start..=idx].trim());
                    }
                }
            }
            _ => {}
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::{
        parse_external_api_expansion, sanitize_generated_artifact_name, ExternalApiExpansion,
        GeneratedDraftArtifact,
    };

    #[test]
    fn parses_json_wrapped_in_code_fence() {
        let expansion = parse_external_api_expansion(
            r##"```json
{
  "api_name": "",
  "data_drafts": [
    {
      "name": "PaymentIntent",
      "draft_markdown": "# PaymentIntent\n\nDescription"
    }
  ],
  "context_drafts": [
    {
      "name": "stripe_api",
      "draft_markdown": "# Stripe API\n\nDescription"
    }
  ]
}
```"##,
            "stripe",
        )
        .expect("parse");

        assert_eq!(
            expansion,
            ExternalApiExpansion {
                api_name: "stripe".to_string(),
                data_drafts: vec![GeneratedDraftArtifact {
                    name: "PaymentIntent".to_string(),
                    draft_markdown: "# PaymentIntent\n\nDescription".to_string(),
                }],
                context_drafts: vec![GeneratedDraftArtifact {
                    name: "stripe_api".to_string(),
                    draft_markdown: "# Stripe API\n\nDescription".to_string(),
                }],
            }
        );
    }

    #[test]
    fn sanitizes_generated_artifact_names_for_paths() {
        assert_eq!(
            sanitize_generated_artifact_name("Charge/Create Payment"),
            "Charge_Create_Payment"
        );
    }

    #[test]
    fn parses_json_with_preface_text() {
        let expansion = parse_external_api_expansion(
            r##"Here is the generated bundle:

```json
{
  "api_name": "AISStream",
  "data_drafts": [],
  "context_drafts": [
    {
      "name": "aisstream",
      "draft_markdown": "# AISStream"
    }
  ]
}
```"##,
            "aisstream",
        )
        .expect("parse");

        assert_eq!(expansion.api_name, "AISStream");
        assert_eq!(expansion.context_drafts.len(), 1);
    }

    #[test]
    fn parses_first_balanced_json_object_from_wrapped_output() {
        let expansion = parse_external_api_expansion(
            r##"I found the following result:
{
  "api_name": "AISStream",
  "data_drafts": [],
  "context_drafts": [
    {
      "name": "aisstream",
      "draft_markdown": "# AISStream"
    }
  ]
}

Let me know if you want revisions."##,
            "aisstream",
        )
        .expect("parse");

        assert_eq!(expansion.api_name, "AISStream");
        assert_eq!(expansion.context_drafts[0].name, "aisstream");
    }
}
