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
    let question_markers = ["?", "## Questions", "# Questions", "**Questions**"];
    question_markers
        .iter()
        .any(|marker| output.contains(marker))
        && (output.contains("clarification")
            || output.contains("answer")
            || output.contains("question"))
}
