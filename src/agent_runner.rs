use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

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
        Ok(Self {
            api_key,
            model,
            client: reqwest::blocking::Client::new(),
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
            max_tokens: req.max_tokens,
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
    fn system_block_constructors() {
        let plain = SystemBlock::new("hello");
        assert!(!plain.cache);
        assert_eq!(plain.text, "hello");

        let cached = SystemBlock::cached("world");
        assert!(cached.cache);
        assert_eq!(cached.text, "world");
    }
}
