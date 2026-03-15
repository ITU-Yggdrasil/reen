//! Token limiter for API requests, measured in tokens per minute.
//!
//! Uses a token bucket with continuous refill to keep throughput stable.
//! Token count is approximated from text (word count and character-based heuristics).

use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};

/// Approximate tokens per word (typical for English text).
const TOKENS_PER_WORD: f64 = 1.3;
/// Fallback: characters per token (conservative for code/markdown).
const CHARS_PER_TOKEN: usize = 4;
/// Overhead for provider-specific message framing.
const REQUEST_OVERHEAD_TOKENS: usize = 256;

/// Estimates token count for text using word count and character-based heuristics.
/// Conservative to avoid exceeding provider limits.
pub fn estimate_tokens(text: &str) -> usize {
    if text.is_empty() {
        return 1;
    }
    let word_count = text.split_whitespace().count();
    let char_count = text.chars().count();
    let by_words = (word_count as f64 * TOKENS_PER_WORD).ceil() as usize;
    let by_chars = (char_count + CHARS_PER_TOKEN - 1) / CHARS_PER_TOKEN;
    by_words.max(by_chars).max(1)
}

/// Estimates tokens for an API request: main content + serialized additional context + overhead.
pub fn estimate_request_tokens(
    main_content: &str,
    additional_context: &std::collections::HashMap<String, serde_json::Value>,
) -> usize {
    let main = estimate_tokens(main_content);
    let context_str = serde_json::to_string(additional_context).unwrap_or_default();
    let context = estimate_tokens(&context_str);
    (main + context + REQUEST_OVERHEAD_TOKENS).max(1)
}

/// Limits token throughput to at most `tokens_per_minute` per minute.
/// Uses a token bucket with continuous refill for stable, non-bursty throughput.
/// Call `acquire_tokens(estimated)` before each API request.
#[derive(Clone)]
pub struct TokenLimiter {
    inner: Arc<Mutex<TokenLimiterInner>>,
}

struct TokenLimiterInner {
    tokens_available: f64,
    max_tokens: f64,
    refill_per_sec: f64,
    last_refill: Instant,
}

impl TokenLimiter {
    /// Creates a token limiter allowing at most `tokens_per_minute` tokens per minute.
    /// Panics if `tokens_per_minute` is zero or negative.
    pub fn new(tokens_per_minute: f64) -> Self {
        assert!(
            tokens_per_minute > 0.0,
            "tokens_per_minute must be positive"
        );
        let refill_per_sec = tokens_per_minute / 60.0;
        Self {
            inner: Arc::new(Mutex::new(TokenLimiterInner {
                tokens_available: tokens_per_minute,
                max_tokens: tokens_per_minute,
                refill_per_sec,
                last_refill: Instant::now(),
            })),
        }
    }

    /// Refills the bucket based on elapsed time since last refill.
    fn refill(inner: &mut TokenLimiterInner) {
        let elapsed = inner.last_refill.elapsed();
        inner.tokens_available += inner.refill_per_sec * elapsed.as_secs_f64();
        inner.tokens_available = inner.tokens_available.min(inner.max_tokens);
        inner.last_refill = Instant::now();
    }

    /// Waits until the bucket has at least `estimated` tokens, then consumes them.
    /// Call this before each API request with the estimated token count.
    pub async fn acquire_tokens(&self, estimated: usize) {
        if estimated == 0 {
            return;
        }
        let estimated_f = estimated as f64;
        loop {
            let wait_duration = {
                let mut inner = self.inner.lock().await;
                Self::refill(&mut inner);
                if inner.tokens_available >= estimated_f {
                    inner.tokens_available -= estimated_f;
                    break;
                }
                let deficit = estimated_f - inner.tokens_available;
                Duration::from_secs_f64(deficit / inner.refill_per_sec)
            };
            sleep(wait_duration).await;
        }
    }

    /// Returns true when a single request estimate is larger than the configured
    /// per-minute budget and therefore cannot be reliably scheduled.
    pub async fn exceeds_limit(&self, estimated: usize) -> bool {
        let inner = self.inner.lock().await;
        estimated as f64 > inner.max_tokens
    }

    /// Returns a conservative retry delay after a token-based 429.
    pub async fn retry_delay(&self, estimated: usize) -> Duration {
        let inner = self.inner.lock().await;
        let estimate = estimated.max(1) as f64;
        let refill_wait = estimate / inner.refill_per_sec;
        Duration::from_secs_f64(refill_wait.max(60.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_tokens_word_based() {
        let text = "one two three four five";
        assert!(estimate_tokens(text) >= 5);
    }

    #[test]
    fn estimate_tokens_empty() {
        assert_eq!(estimate_tokens(""), 1);
    }

    #[test]
    fn estimate_request_tokens_includes_overhead() {
        let mut ctx = std::collections::HashMap::new();
        ctx.insert("key".to_string(), serde_json::json!("value"));
        let n = estimate_request_tokens("short", &ctx);
        assert!(n > REQUEST_OVERHEAD_TOKENS);
    }
}
