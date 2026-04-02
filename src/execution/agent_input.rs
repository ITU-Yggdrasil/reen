use serde::Serialize;
use std::collections::HashMap;

#[derive(Serialize)]
pub struct AgentInput {
    #[serde(skip_serializing_if = "Option::is_none")]
    draft_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    context_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    openapi_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    documentation_urls: Option<Vec<String>>,
    #[serde(flatten)]
    additional: HashMap<String, serde_json::Value>,
}

fn json_value_to_string(value: serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(s) => Some(s),
        serde_json::Value::Null => None,
        other => Some(other.to_string()),
    }
}

fn json_value_to_string_vec(value: serde_json::Value) -> Option<Vec<String>> {
    match value {
        serde_json::Value::Array(items) => {
            let values = items
                .into_iter()
                .filter_map(json_value_to_string)
                .collect::<Vec<_>>();
            if values.is_empty() {
                None
            } else {
                Some(values)
            }
        }
        serde_json::Value::String(s) => Some(vec![s]),
        serde_json::Value::Null => None,
        other => Some(vec![other.to_string()]),
    }
}

pub fn build_agent_input(
    agent_name: &str,
    input: &str,
    mut additional_context: HashMap<String, serde_json::Value>,
) -> AgentInput {
    let openapi_content = additional_context
        .remove("openapi_content")
        .and_then(json_value_to_string);
    let documentation_urls = additional_context
        .remove("documentation_urls")
        .and_then(json_value_to_string_vec);

    match agent_name {
        "create_specifications"
        | "create_specifications_context"
        | "create_specifications_data"
        | "create_specifications_main"
        | "create_specifications_external_api" => AgentInput {
            draft_content: Some(input.to_string()),
            context_content: None,
            openapi_content,
            documentation_urls,
            additional: additional_context,
        },
        "create_implementation" | "create_test" => AgentInput {
            draft_content: None,
            context_content: Some(input.to_string()),
            openapi_content: None,
            documentation_urls: None,
            additional: additional_context,
        },
        "fix_draft_blockers" => AgentInput {
            draft_content: None,
            context_content: None,
            openapi_content,
            documentation_urls,
            additional: additional_context,
        },
        _ => AgentInput {
            draft_content: Some(input.to_string()),
            context_content: None,
            openapi_content,
            documentation_urls,
            additional: additional_context,
        },
    }
}

pub fn output_contains_questions(output: &str) -> bool {
    let normalized = output.trim();
    if normalized.is_empty() {
        return false;
    }

    let mut lines = normalized.lines().map(str::trim);
    let Some(first_non_empty) = lines.find(|line| !line.is_empty()) else {
        return false;
    };

    let heading = first_non_empty.to_ascii_lowercase();
    let is_questions_heading =
        heading == "## questions" || heading == "# questions" || heading == "**questions**";
    if !is_questions_heading {
        return false;
    }

    lines.any(|line| {
        let trimmed = line.trim_start();
        let mut chars = trimmed.chars().peekable();
        let mut saw_digit = false;

        while matches!(chars.peek(), Some(c) if c.is_ascii_digit()) {
            saw_digit = true;
            chars.next();
        }

        saw_digit && matches!(chars.next(), Some('.')) && matches!(chars.next(), Some(' '))
    })
}

#[cfg(test)]
mod tests {
    use super::output_contains_questions;

    #[test]
    fn detects_explicit_questions_heading() {
        assert!(output_contains_questions(
            "## Questions\n1. Which endpoint should be used?"
        ));
    }

    #[test]
    fn requires_questions_heading_at_top_level() {
        assert!(!output_contains_questions(
            "Please answer these before I continue.\n1. Should this support retries?"
        ));
    }

    #[test]
    fn ignores_no_questions_summary_text() {
        assert!(!output_contains_questions(
            "Implementation plan complete. There are no questions."
        ));
    }

    #[test]
    fn ignores_regular_output_with_question_word() {
        assert!(!output_contains_questions(
            "This section answers the original question and includes the final implementation."
        ));
    }

    #[test]
    fn ignores_rust_code_with_debug_formatting_and_try_operator() {
        assert!(!output_contains_questions(
            r#"```rust
fn render(value: Result<Option<String>, std::io::Error>) -> Result<(), std::io::Error> {
    tracing::debug!("value={:?}", value);
    let scale = 1.0_f32;
    let inner = value?;
    println!("{:?} {}", inner, scale);
    Ok(())
}
```"#
        ));
    }

    #[test]
    fn ignores_heading_inside_code_block() {
        assert!(!output_contains_questions(
            r#"```md
## Questions
1. This is an example inside fenced code.
```"#
        ));
    }

    #[test]
    fn requires_numbered_question_after_heading() {
        assert!(!output_contains_questions("## Questions\nNo questions."));
    }
}
