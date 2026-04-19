use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Per-request timeout for Anthropic calls. Claude can take several minutes to finish a large
/// non-streaming response (the compile-fix agent sometimes asks for tens of thousands of
/// output tokens when it rewrites multiple files), and the default reqwest blocking timeout is
/// not generous enough. Ten minutes matches Anthropic's own published upper bound for
/// non-streaming Messages calls.
const AGENT_REQUEST_TIMEOUT: Duration = Duration::from_secs(600);

#[derive(Debug, Serialize)]
struct MessagesRequest {
    model: String,
    max_tokens: u32,
    system: Vec<SystemBlockSer>,
    messages: Vec<ApiMessage>,
    temperature: f32,
}

#[derive(Debug, Serialize)]
struct SystemBlockSer {
    r#type: &'static str,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_control: Option<CacheControlSer>,
}

#[derive(Debug, Serialize)]
struct CacheControlSer {
    r#type: &'static str,
}

#[derive(Debug, Serialize)]
struct ApiMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct MessagesResponse {
    content: Vec<ContentBlock>,
    /// Why generation stopped. Values we care about:
    /// - `"end_turn"`: model finished naturally.
    /// - `"max_tokens"`: hit the request's `max_tokens` cap — the response is almost certainly
    ///   truncated and callers will fail to parse it.
    /// - `"stop_sequence"` / `"tool_use"` / missing: treat as non-truncating.
    #[serde(default)]
    stop_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ContentBlock {
    text: Option<String>,
}

#[derive(Clone)]
pub struct SystemBlock {
    pub text: String,
    pub cache: bool,
}

impl SystemBlock {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            cache: false,
        }
    }

    pub fn cached(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            cache: true,
        }
    }
}

pub struct AgentRequest<'a> {
    pub system: Vec<SystemBlock>,
    pub user_content: &'a str,
    pub temperature: f32,
    pub max_tokens: u32,
}

pub struct AgentRunner {
    api_key: String,
    model: String,
    client: reqwest::blocking::Client,
}

impl AgentRunner {
    /// Build a runner from environment variables.
    ///
    /// Reads `ANTHROPIC_API_KEY` (required) and `REEN_MODEL` (optional,
    /// defaults to `claude-sonnet-4-20250514`).
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("ANTHROPIC_API_KEY").context(
            "ANTHROPIC_API_KEY is required. Set it in your environment or in a .env file.",
        )?;
        let model =
            std::env::var("REEN_MODEL").unwrap_or_else(|_| "claude-sonnet-4-20250514".to_string());
        let client = reqwest::blocking::Client::builder()
            .timeout(AGENT_REQUEST_TIMEOUT)
            .build()
            .context("Failed to build HTTP client for Anthropic API")?;
        Ok(Self {
            api_key,
            model,
            client,
        })
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    /// Send a request to the Anthropic Messages API and return the raw
    /// text content from the first content block.
    pub fn run(&self, req: &AgentRequest) -> Result<String> {
        let system_blocks: Vec<SystemBlockSer> = req
            .system
            .iter()
            .map(|block| SystemBlockSer {
                r#type: "text",
                text: block.text.clone(),
                cache_control: if block.cache {
                    Some(CacheControlSer {
                        r#type: "ephemeral",
                    })
                } else {
                    None
                },
            })
            .collect();

        let body = MessagesRequest {
            model: self.model.clone(),
            max_tokens: clamp_max_output_tokens(&self.model, req.max_tokens),
            system: system_blocks,
            messages: vec![ApiMessage {
                role: "user".to_string(),
                content: req.user_content.to_string(),
            }],
            temperature: req.temperature,
        };

        let response = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .context("Failed to send request to Anthropic API")?;

        let status = response.status();
        if !status.is_success() {
            let text = response.text().unwrap_or_default();
            bail!(
                "Anthropic API returned status {}: {}",
                status,
                text.chars().take(500).collect::<String>()
            );
        }

        let parsed: MessagesResponse = response
            .json()
            .context("Failed to parse Anthropic API response")?;

        let requested = clamp_max_output_tokens(&self.model, req.max_tokens);
        if let Some(msg) = truncation_error_message(parsed.stop_reason.as_deref(), requested) {
            bail!(msg);
        }

        parsed
            .content
            .into_iter()
            .find_map(|block| block.text)
            .ok_or_else(|| anyhow::anyhow!("Anthropic API returned no text content"))
    }

    /// Convenience: run a request and extract JSON from the response,
    /// stripping any markdown fences the model may have wrapped around it.
    pub fn run_json(&self, req: &AgentRequest) -> Result<String> {
        let raw = self.run(req)?;
        Ok(extract_json(&raw))
    }
}

/// Clamp a caller's requested `max_tokens` to the per-model output cap published by Anthropic.
///
/// The API rejects the request with HTTP 400 (`max_tokens: N > CAP, which is the maximum allowed
/// number of output tokens for <model>`) if the value exceeds the model's cap. Callers in this
/// crate request generous limits to give long compile-fix rewrites room to land, so we clamp
/// centrally rather than making each agent hard-code the right ceiling.
///
/// Caps below match the published Anthropic Messages API limits as of 2026-Q2. If the model
/// name doesn't match a known family, we fall back to a conservative 4,096 — which matches the
/// historical default that every other agent in this repo uses — so unknown models degrade
/// toward "works but short" rather than "400 Bad Request".
pub(crate) fn clamp_max_output_tokens(model: &str, requested: u32) -> u32 {
    let cap = model_max_output_tokens(model);
    requested.min(cap)
}

/// If the Anthropic response's `stop_reason` indicates the model was cut off because it hit the
/// request's `max_tokens` cap, return an actionable error message. Returning the truncated text
/// to downstream code produces very confusing failures (JSON parsers see partial strings,
/// diff appliers see dangling hunks) — we'd rather fail loudly and name the knob to turn.
///
/// `stop_reason` values documented by Anthropic:
/// - `"end_turn"` / missing → normal completion, no error.
/// - `"max_tokens"` → truncation, surface error.
/// - `"stop_sequence"` / `"tool_use"` / anything else → treat as non-truncating for our purposes.
fn truncation_error_message(
    stop_reason: Option<&str>,
    requested_max_tokens: u32,
) -> Option<String> {
    if stop_reason == Some("max_tokens") {
        Some(format!(
            "Anthropic API returned a truncated response (stop_reason=\"max_tokens\", \
             max_tokens={requested_max_tokens}). Raise the caller's `max_tokens` or split the \
             task into smaller steps."
        ))
    } else {
        None
    }
}

fn model_max_output_tokens(model: &str) -> u32 {
    let m = model.to_ascii_lowercase();
    // Order matters: more specific prefixes first.
    if m.contains("opus-4") {
        return 32_000;
    }
    if m.contains("sonnet-4-5") || m.contains("sonnet-4.5") {
        return 64_000;
    }
    if m.contains("sonnet-4") {
        return 64_000;
    }
    if m.contains("haiku-4") {
        return 64_000;
    }
    if m.contains("3-5-sonnet") || m.contains("3-5-haiku") {
        return 8_192;
    }
    if m.contains("3-opus") {
        return 4_096;
    }
    // Unknown model — use the historically-safe default used by other agents in this crate.
    4_096
}

/// Strip markdown fences that models sometimes wrap around JSON output.
pub fn extract_json(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return trimmed.to_string();
    }
    if let Some(start) = trimmed.find("```json") {
        let after = &trimmed[start + 7..];
        if let Some(end) = after.find("```") {
            return after[..end].trim().to_string();
        }
    }
    if let Some(start) = trimmed.find("```") {
        let after = &trimmed[start + 3..];
        let after = after.trim_start_matches(|c: char| c.is_alphabetic() || c == '\n');
        if let Some(end) = after.find("```") {
            return after[..end].trim().to_string();
        }
    }
    trimmed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_json_plain_object() {
        let input = r#"{"fixes": []}"#;
        assert_eq!(extract_json(input), r#"{"fixes": []}"#);
    }

    #[test]
    fn extract_json_plain_array() {
        let input = r#"[1, 2, 3]"#;
        assert_eq!(extract_json(input), r#"[1, 2, 3]"#);
    }

    #[test]
    fn extract_json_from_code_fence() {
        let input = "Here is the result:\n```json\n{\"fixes\": []}\n```\n";
        assert_eq!(extract_json(input), r#"{"fixes": []}"#);
    }

    #[test]
    fn extract_json_from_bare_fence() {
        let input = "```\n{\"a\":1}\n```";
        assert_eq!(extract_json(input), r#"{"a":1}"#);
    }

    #[test]
    fn truncation_error_message_fires_on_max_tokens_stop_reason() {
        // Regression: before we surfaced stop_reason explicitly, a truncated Anthropic response
        // produced a confusing "expected `,` or `}` at line 1 column 9770" JSON parse failure
        // downstream. This helper must return a clear message the moment truncation happens.
        let err = truncation_error_message(Some("max_tokens"), 16_384)
            .expect("max_tokens stop_reason must produce an error");
        assert!(
            err.contains("stop_reason=\"max_tokens\""),
            "error must name the stop_reason so users can grep for it: {err}"
        );
        assert!(
            err.contains("max_tokens=16384"),
            "error must echo the cap that was hit: {err}"
        );
    }

    #[test]
    fn truncation_error_message_silent_on_normal_completions() {
        // Any non-truncating stop_reason (or a missing one, which older API versions used) must
        // return None so the response flows through to the caller unchanged.
        assert!(truncation_error_message(Some("end_turn"), 16_384).is_none());
        assert!(truncation_error_message(Some("stop_sequence"), 16_384).is_none());
        assert!(truncation_error_message(Some("tool_use"), 16_384).is_none());
        assert!(truncation_error_message(None, 16_384).is_none());
    }

    #[test]
    fn messages_response_deserializes_stop_reason() {
        // Guard against a future rename of the Anthropic wire field, which would silently
        // disable the truncation detector without breaking any other test.
        let payload = r#"{
            "content": [{"type": "text", "text": "hi"}],
            "stop_reason": "max_tokens"
        }"#;
        let parsed: MessagesResponse = serde_json::from_str(payload).unwrap();
        assert_eq!(parsed.stop_reason.as_deref(), Some("max_tokens"));
    }

    #[test]
    fn messages_response_stop_reason_is_optional() {
        // Older API shapes omit `stop_reason`. Deserialization must still succeed.
        let payload = r#"{"content": [{"type": "text", "text": "hi"}]}"#;
        let parsed: MessagesResponse = serde_json::from_str(payload).unwrap();
        assert!(parsed.stop_reason.is_none());
    }

    #[test]
    fn clamp_max_output_tokens_enforces_sonnet_4_cap() {
        // Regression: Anthropic rejects requests > 64000 output tokens for sonnet-4 models with
        // an HTTP 400. The shared runner must clamp before sending so every agent is protected.
        assert_eq!(
            clamp_max_output_tokens("claude-sonnet-4-20250514", 65_536),
            64_000
        );
        assert_eq!(
            clamp_max_output_tokens("claude-sonnet-4-5-20251022", 65_536),
            64_000
        );
    }

    #[test]
    fn clamp_max_output_tokens_enforces_opus_4_cap() {
        assert_eq!(
            clamp_max_output_tokens("claude-opus-4-20250514", 65_536),
            32_000
        );
    }

    #[test]
    fn clamp_max_output_tokens_passes_through_when_under_cap() {
        assert_eq!(
            clamp_max_output_tokens("claude-sonnet-4-20250514", 4_096),
            4_096
        );
        assert_eq!(
            clamp_max_output_tokens("claude-opus-4-20250514", 16_000),
            16_000
        );
    }

    #[test]
    fn clamp_max_output_tokens_falls_back_to_conservative_default_for_unknown_models() {
        // Safety net: if someone points REEN_MODEL at a brand-new model name we don't recognize,
        // we'd rather produce a truncated-but-valid response than a 400 Bad Request.
        assert_eq!(clamp_max_output_tokens("claude-future-99", 65_536), 4_096);
    }

    #[test]
    fn system_block_constructors() {
        let plain = SystemBlock::new("hello");
        assert!(!plain.cache);
        assert_eq!(plain.text, "hello");

        let cached = SystemBlock::cached("world");
        assert!(cached.cache);
        assert_eq!(cached.text, "world");
    }
}
