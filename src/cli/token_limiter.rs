//! Token limiter for API requests, measured in tokens per minute.
//!
//! Uses a token bucket with continuous refill to keep throughput stable.
//! Token count is approximated from text (word count and character-based heuristics).

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;
use tokio::time::Duration;

/// Approximate tokens per word (typical for English text).
pub const TOKENS_PER_WORD: f64 = 1.3;
/// Fallback: characters per token (conservative for code/markdown).
pub const CHARS_PER_TOKEN: usize = 4;
/// Overhead for provider-specific message framing.
pub const REQUEST_OVERHEAD_TOKENS: usize = 256;

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
    max_tokens: f64,
    requests: VecDeque<TokenReservation>,
}

struct TokenReservation {
    timestamp: Instant,
    tokens: f64,
}

impl TokenLimiter {
    /// Creates a token limiter allowing at most `tokens_per_minute` tokens per minute.
    /// Panics if `tokens_per_minute` is zero or negative.
    pub fn new(tokens_per_minute: f64) -> Self {
        assert!(
            tokens_per_minute > 0.0,
            "tokens_per_minute must be positive"
        );
        Self {
            inner: Arc::new(Mutex::new(TokenLimiterInner {
                max_tokens: tokens_per_minute,
                requests: VecDeque::new(),
            })),
        }
    }

    fn prune_expired(inner: &mut TokenLimiterInner, now: Instant) {
        while let Some(front) = inner.requests.front() {
            if now.duration_since(front.timestamp) < Duration::from_secs(60) {
                break;
            }
            inner.requests.pop_front();
        }
    }

    fn used_tokens(inner: &TokenLimiterInner) -> f64 {
        inner.requests.iter().map(|entry| entry.tokens).sum()
    }

    fn wait_time_for(inner: &TokenLimiterInner, estimated_f: f64, now: Instant) -> Duration {
        let mut used = Self::used_tokens(inner);
        if used + estimated_f <= inner.max_tokens {
            return Duration::from_secs(0);
        }

        for entry in &inner.requests {
            used -= entry.tokens;
            if used + estimated_f <= inner.max_tokens {
                let expires_at = entry.timestamp + Duration::from_secs(60);
                return expires_at.saturating_duration_since(now);
            }
        }

        Duration::from_secs(60)
    }

    /// Blocking variant of `acquire_tokens` for synchronous execution paths.
    pub fn acquire_tokens_blocking(&self, estimated: usize) {
        if estimated == 0 {
            return;
        }
        let estimated_f = estimated as f64;
        loop {
            let wait_duration = {
                let mut inner = self.inner.blocking_lock();
                let now = Instant::now();
                Self::prune_expired(&mut inner, now);
                if Self::used_tokens(&inner) + estimated_f <= inner.max_tokens {
                    inner.requests.push_back(TokenReservation {
                        timestamp: now,
                        tokens: estimated_f,
                    });
                    break;
                }
                Self::wait_time_for(&inner, estimated_f, now)
            };
            std::thread::sleep(wait_duration);
        }
    }

    /// Returns true when a single request estimate is larger than the configured
    /// per-minute budget and therefore cannot be reliably scheduled.
    pub async fn exceeds_limit(&self, estimated: usize) -> bool {
        let inner = self.inner.lock().await;
        estimated as f64 > inner.max_tokens
    }

    /// Blocking variant of `exceeds_limit` for synchronous execution paths.
    pub fn exceeds_limit_blocking(&self, estimated: usize) -> bool {
        let inner = self.inner.blocking_lock();
        estimated as f64 > inner.max_tokens
    }

    /// Returns a conservative retry delay after a token-based 429.
    pub async fn retry_delay(&self, estimated: usize) -> Duration {
        let mut inner = self.inner.lock().await;
        let now = Instant::now();
        Self::prune_expired(&mut inner, now);
        let estimate = estimated.max(1) as f64;
        let wait = Self::wait_time_for(&inner, estimate, now);
        if wait.is_zero() {
            Duration::from_secs(1)
        } else {
            wait
        }
    }

    /// Adds already-consumed tokens to the rolling window.
    pub fn add_tokens_blocking(&self, tokens: usize) {
        if tokens == 0 {
            return;
        }
        let mut inner = self.inner.blocking_lock();
        let now = Instant::now();
        Self::prune_expired(&mut inner, now);
        inner.requests.push_back(TokenReservation {
            timestamp: now,
            tokens: tokens as f64,
        });
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
