use html2text::from_read;
use reqwest::blocking::{Client, Response};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::env;
use std::io::Cursor;
use std::sync::Once;
use std::time::Duration;

static DOTENV_INIT: Once = Once::new();

const LEGACY_USER_PROMPT: &str = "Please complete the task described in the system prompt.";

pub fn execute_request(request: &Value) -> Result<String, String> {
    ensure_dotenv_loaded();

    let normalized = normalize_request(request)?;
    let (provider, model_name) = determine_provider(&normalized.model);

    match provider.as_str() {
        "anthropic" => execute_with_anthropic(
            &model_name,
            &normalized.system_content,
            &normalized.user_content,
            normalized.max_output_tokens,
            normalized.tools.as_deref(),
            normalized.tool_context.as_ref(),
        ),
        "ollama" => execute_with_ollama(
            &model_name,
            &normalized.system_content,
            &normalized.user_content,
        ),
        "openai" => execute_with_openai(
            &model_name,
            &normalized.system_content,
            &normalized.user_content,
            normalized.agent_name.as_deref(),
            normalized.tools.as_deref(),
            normalized.tool_context.as_ref(),
        ),
        "mistral" => execute_with_mistral(
            &model_name,
            &normalized.system_content,
            &normalized.user_content,
            normalized.tools.as_deref(),
            normalized.tool_context.as_ref(),
        ),
        _ => Err(format!("Unknown provider: {provider}")),
    }
}

#[derive(Debug, Clone, PartialEq)]
struct NormalizedRequest {
    model: String,
    system_content: String,
    user_content: String,
    agent_name: Option<String>,
    max_output_tokens: Option<u64>,
    tools: Option<Vec<Value>>,
    tool_context: Option<Value>,
}

fn normalize_request(request: &Value) -> Result<NormalizedRequest, String> {
    let model = request
        .get("model")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Missing required field: model".to_string())?
        .to_string();

    let static_prompt = request.get("static_prompt").and_then(Value::as_str);
    let variable_prompt = request.get("variable_prompt").and_then(Value::as_str);
    let system_prompt = request.get("system_prompt").and_then(Value::as_str);

    let (system_content, user_content) =
        if let (Some(static_prompt), Some(variable_prompt)) = (static_prompt, variable_prompt) {
            (static_prompt.to_string(), variable_prompt.to_string())
        } else if let Some(system_prompt) = system_prompt {
            (system_prompt.to_string(), LEGACY_USER_PROMPT.to_string())
        } else {
            return Err(
                "Missing required fields: (system_prompt) or (static_prompt + variable_prompt)"
                    .to_string(),
            );
        };

    Ok(NormalizedRequest {
        model,
        system_content,
        user_content,
        agent_name: request
            .get("agent_name")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        max_output_tokens: request.get("max_output_tokens").and_then(Value::as_u64),
        tools: request
            .get("tools")
            .and_then(Value::as_array)
            .map(|items| items.to_vec()),
        tool_context: request.get("tool_context").cloned(),
    })
}

fn determine_provider(model: &str) -> (String, String) {
    if let Some((provider, model_name)) = model.split_once('/') {
        return (provider.to_lowercase(), model_name.to_string());
    }

    let model_lower = model.to_lowercase();
    if model_lower.contains("claude") || model_lower.contains("anthropic") {
        ("anthropic".to_string(), model.to_string())
    } else if ["gpt", "openai", "o1", "o3"]
        .iter()
        .any(|needle| model_lower.contains(needle))
    {
        ("openai".to_string(), model.to_string())
    } else if model_lower.contains("mistral/") {
        ("mistral".to_string(), model.to_string())
    } else if [
        "ollama",
        "qwen",
        "llama",
        "mistral",
        "phi",
        "gemma",
        "codellama",
    ]
    .iter()
    .any(|needle| model_lower.contains(needle))
    {
        ("ollama".to_string(), model.to_string())
    } else {
        ("ollama".to_string(), model.to_string())
    }
}

fn ensure_dotenv_loaded() {
    DOTENV_INIT.call_once(|| {
        if let Ok(cwd) = env::current_dir() {
            if let Some(dotenv_path) = find_upwards(cwd.as_path(), ".env") {
                let _ = dotenvy::from_path(dotenv_path);
            }
        }
    });
}

fn find_upwards(start: &std::path::Path, name: &str) -> Option<std::path::PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        let candidate = current.join(name);
        if candidate.exists() {
            return Some(candidate);
        }
        if !current.pop() {
            return None;
        }
    }
}

fn resolve_max_output_tokens(request_value: Option<u64>, env_var: &str, default: u64) -> u64 {
    if let Some(request_value) = request_value {
        return request_value.max(1);
    }
    if let Some(env_value) = env::var(env_var).ok() {
        if let Ok(parsed) = env_value.parse::<u64>() {
            return parsed.max(1);
        }
    }
    default
}

fn execute_with_openai(
    model: &str,
    system_content: &str,
    user_content: &str,
    agent_name: Option<&str>,
    tools: Option<&[Value]>,
    tool_context: Option<&Value>,
) -> Result<String, String> {
    let api_key =
        env::var("OPENAI_API_KEY").map_err(|_| "OPENAI_API_KEY environment variable not set".to_string())?;
    let base_url = env::var("OPENAI_BASE_URL").unwrap_or_else(|_| "https://api.openai.com/v1".to_string());
    let timeout = env::var("OPENAI_TIMEOUT_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(180);

    execute_openai_compatible(
        &build_client(timeout)?,
        &format!("{}/chat/completions", base_url.trim_end_matches('/')),
        &api_key,
        model,
        system_content,
        user_content,
        tools,
        tool_context,
        agent_name,
        true,
    )
}

fn execute_with_mistral(
    model: &str,
    system_content: &str,
    user_content: &str,
    tools: Option<&[Value]>,
    tool_context: Option<&Value>,
) -> Result<String, String> {
    let api_key =
        env::var("MISTRAL_API_KEY").map_err(|_| "MISTRAL_API_KEY environment variable not set".to_string())?;
    let base_url =
        env::var("MISTRAL_BASE_URL").unwrap_or_else(|_| "https://api.mistral.ai/v1".to_string());
    let timeout = env::var("MISTRAL_TIMEOUT_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(180);

    execute_openai_compatible(
        &build_client(timeout)?,
        &format!("{}/chat/completions", base_url.trim_end_matches('/')),
        &api_key,
        model,
        system_content,
        user_content,
        tools,
        tool_context,
        None,
        false,
    )
}

fn execute_openai_compatible(
    client: &Client,
    endpoint: &str,
    api_key: &str,
    model: &str,
    system_content: &str,
    user_content: &str,
    tools: Option<&[Value]>,
    tool_context: Option<&Value>,
    agent_name: Option<&str>,
    use_prompt_cache: bool,
) -> Result<String, String> {
    let mut messages = vec![
        json!({"role": "system", "content": system_content}),
        json!({"role": "user", "content": user_content}),
    ];
    let openai_tools = convert_tools_for_openai(tools);

    loop {
        let mut body = json!({
            "model": model,
            "messages": messages,
        });
        if !openai_tools.is_empty() {
            body["tools"] = Value::Array(openai_tools.clone());
            body["tool_choice"] = Value::String("auto".to_string());
        }
        if use_prompt_cache && system_content.len() >= 256 {
            if let Some(agent_name) = agent_name {
                body["prompt_cache_key"] =
                    Value::String(openai_prompt_cache_key(agent_name, system_content));
                if openai_supports_extended_cache(model) {
                    body["prompt_cache_retention"] = Value::String("24h".to_string());
                }
            }
        }

        let response = post_json_bearer(client, endpoint, api_key, &body)?;
        let message = response
            .pointer("/choices/0/message")
            .ok_or_else(|| format!("OpenAI-compatible response missing choices[0].message: {response}"))?;

        let tool_calls = message
            .get("tool_calls")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if openai_tools.is_empty() || tool_calls.is_empty() {
            return Ok(extract_openai_message_content(message.get("content")));
        }

        messages.push(json!({
            "role": "assistant",
            "content": message.get("content").cloned().unwrap_or(Value::Null),
            "tool_calls": tool_calls,
        }));

        for tool_call in tool_calls {
            let tool_call_id = tool_call
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("OpenAI-compatible tool call missing id: {tool_call}"))?;
            let function = tool_call
                .get("function")
                .ok_or_else(|| format!("OpenAI-compatible tool call missing function: {tool_call}"))?;
            let tool_name = function
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("OpenAI-compatible tool call missing function name: {tool_call}"))?;
            let arguments = coerce_tool_arguments(function.get("arguments"))?;
            let result = execute_tool_call(tool_name, &arguments, tool_context)?;
            messages.push(json!({
                "role": "tool",
                "tool_call_id": tool_call_id,
                "content": result,
            }));
        }
    }
}

fn execute_with_anthropic(
    model: &str,
    system_content: &str,
    user_content: &str,
    max_output_tokens: Option<u64>,
    tools: Option<&[Value]>,
    tool_context: Option<&Value>,
) -> Result<String, String> {
    let api_key = env::var("ANTHROPIC_API_KEY")
        .map_err(|_| "ANTHROPIC_API_KEY environment variable not set".to_string())?;
    let base_url =
        env::var("ANTHROPIC_BASE_URL").unwrap_or_else(|_| "https://api.anthropic.com".to_string());
    let timeout = env::var("ANTHROPIC_TIMEOUT_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(180);
    let client = build_client(timeout)?;

    let mut headers = HeaderMap::new();
    headers.insert(
        "x-api-key",
        HeaderValue::from_str(&api_key).map_err(|e| format!("Invalid ANTHROPIC_API_KEY header: {e}"))?,
    );
    headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

    let system_blocks = Value::Array(vec![json!({
        "type": "text",
        "text": system_content,
        "cache_control": { "type": "ephemeral" },
    })]);
    let mut messages = vec![json!({
        "role": "user",
        "content": user_content,
    })];

    loop {
        let mut body = json!({
            "model": model,
            "max_tokens": resolve_max_output_tokens(max_output_tokens, "ANTHROPIC_MAX_OUTPUT_TOKENS", 8096),
            "system": system_blocks,
            "messages": messages,
        });
        if let Some(tools) = tools {
            body["tools"] = Value::Array(tools.to_vec());
        }

        let response = post_json_with_headers(
            &client,
            &format!("{}/v1/messages", base_url.trim_end_matches('/')),
            headers.clone(),
            &body,
        )?;
        let content = response
            .get("content")
            .and_then(Value::as_array)
            .cloned()
            .ok_or_else(|| format!("Anthropic response missing content array: {response}"))?;
        let tool_uses = content
            .iter()
            .filter(|block| block.get("type").and_then(Value::as_str) == Some("tool_use"))
            .cloned()
            .collect::<Vec<_>>();

        if tools.is_none() || tool_uses.is_empty() {
            return Ok(anthropic_text_from_blocks(&content));
        }

        messages.push(json!({
            "role": "assistant",
            "content": Value::Array(content.clone()),
        }));

        let mut tool_results = Vec::new();
        for tool_use in tool_uses {
            let tool_use_id = tool_use
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("Anthropic tool_use missing id: {tool_use}"))?;
            let tool_name = tool_use
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("Anthropic tool_use missing name: {tool_use}"))?;
            let arguments = tool_use.get("input").cloned().unwrap_or_else(|| json!({}));
            let result = execute_tool_call(tool_name, &arguments, tool_context)?;
            tool_results.push(json!({
                "type": "tool_result",
                "tool_use_id": tool_use_id,
                "content": result,
            }));
        }

        messages.push(json!({
            "role": "user",
            "content": Value::Array(tool_results),
        }));
    }
}

fn execute_with_ollama(
    model: &str,
    system_content: &str,
    user_content: &str,
) -> Result<String, String> {
    let base_url =
        env::var("OLLAMA_BASE_URL").unwrap_or_else(|_| "http://localhost:11434".to_string());
    let timeout = env::var("OLLAMA_TIMEOUT_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(180);
    let client = build_client(timeout)?;

    let model_name = model.strip_prefix("ollama:").unwrap_or(model);
    let request_body = json!({
        "model": model_name,
        "stream": false,
        "messages": [
            {"role": "system", "content": system_content},
            {"role": "user", "content": user_content},
        ],
    });
    let endpoint = format!("{}/api/chat", base_url.trim_end_matches('/'));
    let response = post_json(&client, &endpoint, &request_body)?;
    let first_output = response
        .pointer("/message/content")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("Ollama response missing message.content: {response}"))?
        .to_string();

    let lower = first_output.to_lowercase();
    let asks_for_prompt = [
        "provide me with the system prompt",
        "provide the system prompt",
        "provide me with the details of the task",
        "provide details of the task",
    ]
    .iter()
    .any(|needle| lower.contains(needle));
    if !asks_for_prompt {
        return Ok(first_output);
    }

    let fallback_body = json!({
        "model": model_name,
        "stream": false,
        "messages": [{
            "role": "user",
            "content": format!(
                "{system_content}\n\n{user_content}\n\nPlease complete the task described above. Return only the final result."
            ),
        }],
    });
    let fallback = post_json(&client, &endpoint, &fallback_body)?;
    fallback
        .pointer("/message/content")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| format!("Ollama fallback response missing message.content: {fallback}"))
}

fn build_client(timeout_seconds: u64) -> Result<Client, String> {
    Client::builder()
        .timeout(Duration::from_secs(timeout_seconds))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {e}"))
}

fn post_json(client: &Client, url: &str, body: &Value) -> Result<Value, String> {
    decode_json_response(
        client
            .post(url)
            .json(body)
            .send()
            .map_err(|e| format!("Failed to send request to {url}: {e}"))?,
        url,
    )
}

fn post_json_bearer(client: &Client, url: &str, api_key: &str, body: &Value) -> Result<Value, String> {
    decode_json_response(
        client
            .post(url)
            .header(AUTHORIZATION, format!("Bearer {api_key}"))
            .json(body)
            .send()
            .map_err(|e| format!("Failed to send request to {url}: {e}"))?,
        url,
    )
}

fn post_json_with_headers(
    client: &Client,
    url: &str,
    headers: HeaderMap,
    body: &Value,
) -> Result<Value, String> {
    decode_json_response(
        client
            .post(url)
            .headers(headers)
            .json(body)
            .send()
            .map_err(|e| format!("Failed to send request to {url}: {e}"))?,
        url,
    )
}

fn decode_json_response(response: Response, url: &str) -> Result<Value, String> {
    let status = response.status();
    let payload = response
        .text()
        .map_err(|e| format!("Failed to read response body from {url}: {e}"))?;
    if !status.is_success() {
        return Err(format!("HTTP {} for {url}: {}", status.as_u16(), payload));
    }
    serde_json::from_str(&payload)
        .map_err(|e| format!("Failed to parse JSON response from {url}: {e}. Body: {payload}"))
}

fn convert_tools_for_openai(tools: Option<&[Value]>) -> Vec<Value> {
    let mut converted = Vec::new();
    for tool in tools.unwrap_or(&[]) {
        if tool.get("type").and_then(Value::as_str) == Some("function") {
            converted.push(tool.clone());
            continue;
        }
        converted.push(json!({
            "type": "function",
            "function": {
                "name": tool.get("name").cloned().unwrap_or(Value::Null),
                "description": tool.get("description").cloned().unwrap_or_else(|| Value::String(String::new())),
                "parameters": tool.get("input_schema").cloned().unwrap_or_else(|| json!({"type":"object","properties":{}})),
            }
        }));
    }
    converted
}

fn extract_openai_message_content(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|part| match part {
                Value::String(text) => Some(text.clone()),
                Value::Object(map) => {
                    if map.get("type").and_then(Value::as_str) == Some("text") {
                        map.get("text").and_then(Value::as_str).map(ToOwned::to_owned)
                    } else {
                        None
                    }
                }
                _ => None,
            })
            .collect::<String>(),
        _ => String::new(),
    }
}

fn anthropic_text_from_blocks(blocks: &[Value]) -> String {
    let mut output = String::new();
    for block in blocks {
        if block.get("type").and_then(Value::as_str) == Some("text") {
            if let Some(text) = block.get("text").and_then(Value::as_str) {
                output.push_str(text);
            }
        }
    }
    output
}

fn execute_tool_call(
    tool_name: &str,
    arguments: &Value,
    tool_context: Option<&Value>,
) -> Result<String, String> {
    match tool_name {
        "fetch_dependency_artifacts" => execute_fetch_dependency_artifacts(arguments, tool_context),
        "fetch_documentation" => execute_fetch_documentation(arguments, tool_context),
        _ => Err(format!("Unknown tool call: {tool_name}")),
    }
}

fn execute_fetch_dependency_artifacts(
    arguments: &Value,
    tool_context: Option<&Value>,
) -> Result<String, String> {
    let paths = match arguments.get("paths") {
        Some(Value::String(path)) => vec![path.clone()],
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| {
                item.as_str()
                    .map(ToOwned::to_owned)
                    .ok_or_else(|| "fetch_dependency_artifacts requires string paths".to_string())
            })
            .collect::<Result<Vec<_>, _>>()?,
        _ => {
            return Err("fetch_dependency_artifacts requires a string array 'paths'".to_string());
        }
    };

    let dependency_context = tool_context
        .and_then(|value| value.get("dependency_tool_context"))
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let dependency_artifacts = dependency_context
        .get("dependency_artifacts")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let implemented_dependency_artifacts = dependency_context
        .get("implemented_dependency_artifacts")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut found = Vec::new();
    let mut missing = Vec::new();
    for path in paths {
        let artifact = dependency_artifacts
            .iter()
            .chain(implemented_dependency_artifacts.iter())
            .find(|artifact| artifact.get("path").and_then(Value::as_str) == Some(path.as_str()))
            .cloned();
        if let Some(artifact) = artifact {
            found.push(artifact);
        } else {
            missing.push(Value::String(path));
        }
    }

    Ok(json!({
        "artifacts": found,
        "missing_paths": missing,
    })
    .to_string())
}

fn execute_fetch_documentation(
    arguments: &Value,
    tool_context: Option<&Value>,
) -> Result<String, String> {
    let url = arguments
        .get("url")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "fetch_documentation requires a string 'url'".to_string())?;
    if let Some(allowed_urls) = tool_context
        .and_then(|value| value.get("documentation_urls"))
        .and_then(Value::as_array)
    {
        let allowed = allowed_urls.iter().filter_map(Value::as_str).collect::<Vec<_>>();
        if !allowed.is_empty() && !allowed.contains(&url) {
            return Err(format!(
                "Documentation URL not allowed by draft context: {url}"
            ));
        }
    }

    let client = build_client(180)?;
    let response = client
        .get(url)
        .send()
        .map_err(|e| format!("Failed to fetch documentation URL {url}: {e}"))?;
    let status = response.status();
    if !status.is_success() {
        let body = response
            .text()
            .unwrap_or_else(|_| "<response body unavailable>".to_string());
        return Err(format!(
            "Documentation URL returned HTTP {}: {}",
            status.as_u16(),
            body
        ));
    }

    let is_html = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.contains("html"))
        .unwrap_or(false);
    let body = response
        .text()
        .map_err(|e| format!("Failed to read documentation response body: {e}"))?;
    let normalized = if is_html {
        from_read(Cursor::new(body.as_bytes()), 80)
            .map_err(|e| format!("Failed to extract documentation text: {e}"))?
    } else {
        body
    };
    let relevant = select_relevant_text(
        &normalized,
        arguments.get("section_hint").and_then(Value::as_str),
        12_000,
    );

    Ok(json!({
        "url": url,
        "section_hint": arguments.get("section_hint").cloned().unwrap_or(Value::Null),
        "content": relevant,
    })
    .to_string())
}

fn select_relevant_text(text: &str, section_hint: Option<&str>, max_chars: usize) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let Some(section_hint) = section_hint else {
        return trimmed.chars().take(max_chars).collect();
    };

    let terms = section_hint
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|term| term.len() >= 3)
        .map(|term| term.to_lowercase())
        .collect::<Vec<_>>();
    if terms.is_empty() {
        return trimmed.chars().take(max_chars).collect();
    }

    let sections = trimmed
        .split("\n\n")
        .map(str::trim)
        .filter(|chunk| !chunk.is_empty())
        .collect::<Vec<_>>();
    let mut scored = sections
        .iter()
        .enumerate()
        .filter_map(|(index, chunk)| {
            let haystack = chunk.to_lowercase();
            let score = terms
                .iter()
                .map(|term| haystack.matches(term).count())
                .sum::<usize>();
            (score > 0).then_some((score, index, *chunk))
        })
        .collect::<Vec<_>>();

    if scored.is_empty() {
        return trimmed.chars().take(max_chars).collect();
    }

    scored.sort_by(|a, b| b.cmp(a));
    let mut output = String::new();
    for (_, _, chunk) in scored {
        if !output.is_empty() {
            output.push_str("\n\n");
        }
        let remaining = max_chars.saturating_sub(output.len());
        if remaining == 0 {
            break;
        }
        output.extend(chunk.chars().take(remaining));
        if output.len() >= max_chars {
            break;
        }
    }
    output
}

fn coerce_tool_arguments(arguments: Option<&Value>) -> Result<Value, String> {
    match arguments {
        None | Some(Value::Null) => Ok(json!({})),
        Some(Value::Object(_)) => Ok(arguments.cloned().unwrap_or_else(|| json!({}))),
        Some(Value::String(raw)) => {
            if raw.trim().is_empty() {
                Ok(json!({}))
            } else {
                serde_json::from_str(raw)
                    .map_err(|e| format!("Invalid tool argument JSON string: {e}"))
            }
        }
        Some(other) => Err(format!(
            "Unsupported tool argument payload: {}",
            match other {
                Value::Array(_) => "array",
                Value::Bool(_) => "bool",
                Value::Number(_) => "number",
                Value::String(_) => "string",
                Value::Object(_) => "object",
                Value::Null => "null",
            }
        )),
    }
}

fn openai_prompt_cache_key(agent_name: &str, static_prompt: &str) -> String {
    let short_hash = hex::encode(Sha256::digest(static_prompt.as_bytes()));
    format!("reen:{agent_name}:{}", &short_hash[..16])
}

fn openai_supports_extended_cache(model: &str) -> bool {
    let model_lower = model.to_lowercase();
    ["gpt-4.1", "gpt-5", "gpt-5.1", "gpt-5.2", "gpt-5.4", "gp5-5.1"]
        .iter()
        .any(|needle| model_lower.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::{
        coerce_tool_arguments, determine_provider, execute_fetch_dependency_artifacts,
        normalize_request, openai_prompt_cache_key, select_relevant_text,
    };
    use serde_json::json;

    #[test]
    fn normalize_request_supports_split_prompts() {
        let request = json!({
            "model": "gpt-5.4",
            "static_prompt": "system",
            "variable_prompt": "user",
            "agent_name": "create_implementation",
        });

        let normalized = normalize_request(&request).expect("normalize request");
        assert_eq!(normalized.system_content, "system");
        assert_eq!(normalized.user_content, "user");
        assert_eq!(normalized.agent_name.as_deref(), Some("create_implementation"));
    }

    #[test]
    fn normalize_request_supports_legacy_system_prompt() {
        let request = json!({
            "model": "claude-sonnet",
            "system_prompt": "do the thing",
        });

        let normalized = normalize_request(&request).expect("normalize request");
        assert_eq!(normalized.system_content, "do the thing");
        assert_eq!(
            normalized.user_content,
            "Please complete the task described in the system prompt."
        );
    }

    #[test]
    fn provider_detection_prefers_explicit_prefix() {
        assert_eq!(
            determine_provider("mistral/codestral-latest"),
            ("mistral".to_string(), "codestral-latest".to_string())
        );
        assert_eq!(
            determine_provider("gpt-5.4"),
            ("openai".to_string(), "gpt-5.4".to_string())
        );
    }

    #[test]
    fn dependency_tool_returns_artifacts_and_missing_paths() {
        let result = execute_fetch_dependency_artifacts(
            &json!({"paths": ["src/a.rs", "src/missing.rs"]}),
            Some(&json!({
                "dependency_tool_context": {
                    "dependency_artifacts": [
                        {"path": "src/a.rs", "content": "alpha"}
                    ],
                    "implemented_dependency_artifacts": [
                        {"path": "src/b.rs", "content": "beta"}
                    ]
                }
            })),
        )
        .expect("tool call should succeed");
        let parsed: serde_json::Value = serde_json::from_str(&result).expect("valid json");
        assert_eq!(parsed["artifacts"].as_array().map(Vec::len), Some(1));
        assert_eq!(parsed["missing_paths"].as_array().map(Vec::len), Some(1));
    }

    #[test]
    fn select_relevant_text_prioritizes_matching_sections() {
        let text = "\
Authentication\nUse API keys here.\n\nPagination\nUse cursor tokens here.\n\nErrors\nThese are the errors.";
        let selected = select_relevant_text(text, Some("pagination token"), 10_000);
        assert!(selected.contains("Pagination"));
        assert!(!selected.starts_with("Authentication"));
    }

    #[test]
    fn coerce_tool_arguments_accepts_json_strings() {
        let parsed = coerce_tool_arguments(Some(&json!("{\"url\":\"https://example.com\"}")))
            .expect("tool args");
        assert_eq!(parsed["url"].as_str(), Some("https://example.com"));
    }

    #[test]
    fn prompt_cache_key_is_stable() {
        let key = openai_prompt_cache_key("create_test", "static prompt");
        assert!(key.starts_with("reen:create_test:"));
        assert_eq!(key.len(), "reen:create_test:".len() + 16);
    }
}
