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

    let lowercase = normalized.to_ascii_lowercase();

    let explicit_question_section = lowercase.contains("## questions")
        || lowercase.contains("# questions")
        || lowercase.contains("**questions**");
    let asks_for_clarification = lowercase.contains("need clarification")
        || lowercase.contains("needs clarification")
        || lowercase.contains("please answer")
        || lowercase.contains("please provide")
        || lowercase.contains("questions that need answers");
    let has_enumerated_questions = lowercase.contains("1.") && normalized.contains('?');

    explicit_question_section || asks_for_clarification || has_enumerated_questions
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
    fn detects_direct_request_for_answers() {
        assert!(output_contains_questions(
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
}
