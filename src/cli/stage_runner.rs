use anyhow::Result;
use std::future::Future;
use std::sync::Arc;
use tokio::task::block_in_place;
use tokio::time::{Duration, sleep};

use reen::execution::{NativeExecutionControl, NativeRequestStep, NativeStepUsage, TokenLimiter};

use super::Config;
use super::agent_executor::AgentExecutor;
use super::progress::{ProgressIndicator, print_timed_status};
use super::rate_limiter::RateLimiter;

#[derive(Clone)]
pub(crate) struct CliExecutionControl {
    token_limiter: Option<Arc<TokenLimiter>>,
    rate_limiter: Option<Arc<RateLimiter>>,
    verbose: bool,
}

impl CliExecutionControl {
    pub(crate) fn new(
        token_limiter: Option<Arc<TokenLimiter>>,
        rate_limiter: Option<Arc<RateLimiter>>,
        verbose: bool,
    ) -> Self {
        Self {
            token_limiter,
            rate_limiter,
            verbose,
        }
    }

    pub(crate) fn token_limiter(&self) -> Option<&Arc<TokenLimiter>> {
        self.token_limiter.as_ref()
    }

    pub(crate) fn rate_limiter(&self) -> Option<&Arc<RateLimiter>> {
        self.rate_limiter.as_ref()
    }

    pub(crate) fn verbose(&self) -> bool {
        self.verbose
    }
}

impl NativeExecutionControl for CliExecutionControl {
    fn before_model_request(&self, step: &NativeRequestStep) -> Result<(), String> {
        if self.verbose {
            print_timed_status(
                "Submitting request",
                &format!(
                    "{}/{} (~{} input tokens)",
                    step.provider, step.model, step.estimated_input_tokens
                ),
            );
        }
        if let Some(limiter) = &self.token_limiter {
            let exceeds_limit =
                block_in_place(|| limiter.exceeds_limit_blocking(step.estimated_input_tokens));
            if exceeds_limit {
                eprintln!(
                    "Warning: estimated request size ({} input tokens) exceeds configured --token-limit/REEN_TOKEN_LIMIT budget for one minute; continuing request and relying on provider rate-limit handling (429 retry/backoff).",
                    step.estimated_input_tokens
                );
            } else {
                block_in_place(|| limiter.acquire_tokens_blocking(step.estimated_input_tokens));
            }
        }
        if let Some(limiter) = &self.rate_limiter {
            block_in_place(|| limiter.acquire_blocking());
        }
        Ok(())
    }

    fn after_model_response(&self, usage: &NativeStepUsage) {
        if self.verbose {
            let mut details = format!("{}/{}", usage.provider, usage.model);
            if let Some(output_tokens) = usage.output_tokens {
                details.push_str(&format!(" (output {} tokens", output_tokens));
                if let Some(total_tokens) = usage.total_tokens {
                    details.push_str(&format!(", total {}", total_tokens));
                }
                details.push(')');
            } else if let Some(total_tokens) = usage.total_tokens {
                details.push_str(&format!(" (total {} tokens)", total_tokens));
            }
            print_timed_status("Received response", &details);
        }

        let Some(limiter) = &self.token_limiter else {
            return;
        };

        let additional_tokens = if let Some(total_tokens) = usage.total_tokens {
            (total_tokens as usize).saturating_sub(usage.estimated_input_tokens)
        } else {
            let input_delta = usage
                .input_tokens
                .map(|input| (input as usize).saturating_sub(usage.estimated_input_tokens))
                .unwrap_or(0);
            let output_tokens = usage.output_tokens.unwrap_or(0) as usize;
            input_delta.saturating_add(output_tokens)
        };

        block_in_place(|| limiter.add_tokens_blocking(additional_tokens));
    }
}

#[derive(Clone)]
pub(crate) struct ExecutionResources {
    pub(crate) token_limiter: Option<Arc<TokenLimiter>>,
    pub(crate) rate_limiter: Option<Arc<RateLimiter>>,
    pub(crate) execution_control: Option<CliExecutionControl>,
}

impl ExecutionResources {
    pub(crate) fn new(rate_limit: Option<f64>, token_limit: Option<f64>, verbose: bool) -> Self {
        let rate_limiter = rate_limit.map(RateLimiter::new).map(Arc::new);
        let token_limiter = token_limit.map(TokenLimiter::new).map(Arc::new);
        let execution_control = Some(CliExecutionControl::new(
            token_limiter.clone(),
            rate_limiter.clone(),
            verbose,
        ));
        Self {
            token_limiter,
            rate_limiter,
            execution_control,
        }
    }
}

#[derive(Clone)]
pub(crate) struct StageItem<T> {
    pub(crate) name: String,
    pub(crate) estimated: usize,
    pub(crate) cache_hit: bool,
    pub(crate) payload: T,
}

pub(crate) fn estimate_agent_request_tokens(
    executor: &AgentExecutor,
    input: &str,
    additional_context: &std::collections::HashMap<String, serde_json::Value>,
) -> usize {
    executor
        .estimate_request_tokens(input, additional_context.clone())
        .unwrap_or_else(|_| reen::execution::estimate_request_tokens(input, additional_context))
}

pub(crate) fn is_rate_limit_error(error: &anyhow::Error) -> bool {
    let message = error.to_string();
    let lower = message.to_lowercase();
    message.contains("429")
        || lower.contains("rate limit")
        || lower.contains("rate_limit")
        || lower.contains("ratelimit")
}

fn parse_server_retry_delay(error: &anyhow::Error) -> Option<Duration> {
    let message = error.to_string().to_lowercase();

    for marker in [
        "retry-after:",
        "retry_after:",
        "\"retry_after\":",
        "'retry_after':",
        "retry-after=",
        "retry_after=",
        "x-ratelimit-reset-requests:",
        "x-ratelimit-reset-tokens:",
        "x-ratelimit-reset:",
        "ratelimit-reset:",
        "retry-after-ms:",
        "try again in ",
        "retry in ",
        "please try again in ",
    ] {
        if let Some(index) = message.find(marker) {
            let remainder = &message[index + marker.len()..];
            if let Some(duration) = parse_duration_prefix(remainder) {
                return Some(duration);
            }
        }
    }
    None
}

fn parse_duration_prefix(input: &str) -> Option<Duration> {
    let trimmed = input
        .trim_start_matches(|c: char| c.is_whitespace() || c == '"' || c == '\'')
        .trim_start();
    let mut number = String::new();
    let mut chars = trimmed.chars().peekable();
    while let Some(ch) = chars.peek() {
        if ch.is_ascii_digit() || *ch == '.' {
            number.push(*ch);
            chars.next();
        } else {
            break;
        }
    }
    if number.is_empty() {
        return None;
    }
    while let Some(ch) = chars.peek() {
        if ch.is_whitespace() {
            chars.next();
        } else {
            break;
        }
    }
    let mut unit = String::new();
    while let Some(ch) = chars.peek() {
        if ch.is_ascii_alphabetic() || *ch == '-' {
            unit.push(*ch);
            chars.next();
        } else {
            break;
        }
    }
    let value = number.parse::<f64>().ok()?;
    if !value.is_finite() || value < 0.0 {
        return None;
    }
    if value == 0.0 {
        return Some(Duration::from_secs(1));
    }
    let seconds = match unit.as_str() {
        "" | "s" | "sec" | "secs" | "second" | "seconds" => value,
        "ms" | "msec" | "msecs" | "millisecond" | "milliseconds" => value / 1000.0,
        "m" | "min" | "mins" | "minute" | "minutes" => value * 60.0,
        "h" | "hr" | "hrs" | "hour" | "hours" => value * 3600.0,
        _ => return None,
    };
    Some(Duration::from_secs_f64(seconds.max(1.0)))
}

pub(crate) async fn acquire_request_capacity(
    token_limiter: Option<&Arc<TokenLimiter>>,
    rate_limiter: Option<&Arc<RateLimiter>>,
    estimated: usize,
) -> Result<()> {
    if let Some(limiter) = token_limiter {
        if limiter.exceeds_limit(estimated).await {
            eprintln!(
                "Warning: estimated request size ({estimated} input tokens) exceeds configured --token-limit/REEN_TOKEN_LIMIT budget for one minute; continuing request and relying on provider rate-limit handling (429 retry/backoff)."
            );
        }
    }
    if let Some(limiter) = rate_limiter {
        let _ = limiter;
    }
    Ok(())
}

pub(crate) async fn prepare_rate_limit_retry(
    error: &anyhow::Error,
    item_name: &str,
    estimated: usize,
    token_limiter: Option<&Arc<TokenLimiter>>,
    rate_limiter: Option<&Arc<RateLimiter>>,
) -> bool {
    let server_delay = parse_server_retry_delay(error);
    let mut waited = false;
    if let Some(limiter) = token_limiter {
        let limiter_delay = limiter.retry_delay(estimated).await;
        let delay = server_delay
            .map(|value| value.max(limiter_delay))
            .unwrap_or(limiter_delay);
        eprintln!(
            "Rate limit (429) exceeded for {}, waiting {}s before retrying...",
            item_name,
            delay.as_secs()
        );
        sleep(delay).await;
        waited = true;
    }
    if let Some(limiter) = rate_limiter {
        if !waited {
            let fallback_delay = limiter.retry_delay();
            let delay = server_delay
                .map(|value| value.max(fallback_delay))
                .unwrap_or(fallback_delay);
            eprintln!(
                "Rate limit (429) exceeded for {}, waiting {}s and retrying with slower rate...",
                item_name,
                delay.as_secs()
            );
            sleep(delay).await;
            waited = true;
        }
        limiter.back_off().await;
    }
    if !waited {
        if let Some(delay) = server_delay {
            eprintln!(
                "Rate limit (429) exceeded for {}, waiting {}s before retrying...",
                item_name,
                delay.as_secs()
            );
            sleep(delay).await;
            waited = true;
        }
    }
    waited
}

async fn await_with_heartbeat<R, Fut>(item_name: &str, verbose: bool, future: Fut) -> Result<R>
where
    Fut: Future<Output = Result<R>> + Send + 'static,
    R: Send + 'static,
{
    if !verbose {
        return future.await;
    }

    let heartbeat_interval = Duration::from_secs(15);
    let started_at = std::time::Instant::now();
    let mut future_handle = tokio::spawn(future);

    loop {
        let heartbeat = sleep(heartbeat_interval);
        tokio::pin!(heartbeat);
        tokio::select! {
            result = &mut future_handle => {
                return result.map_err(|error| anyhow::anyhow!("Stage task join error: {}", error))?;
            }
            _ = &mut heartbeat => {
                if future_handle.is_finished() {
                    continue;
                }
                print_timed_status(
                    "Still processing",
                    &format!("{} ({}s elapsed)", item_name, started_at.elapsed().as_secs()),
                );
            }
        }
    }
}

async fn run_stage_item<T, R, F, Fut>(
    item: StageItem<T>,
    resources: ExecutionResources,
    process: Arc<F>,
) -> Result<R>
where
    T: Clone + Send + 'static,
    R: Send + 'static,
    F: Fn(T, Option<CliExecutionControl>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<R>> + Send + 'static,
{
    if !item.cache_hit {
        acquire_request_capacity(
            resources.token_limiter.as_ref(),
            resources.rate_limiter.as_ref(),
            item.estimated,
        )
        .await?;
    }

    let verbose = resources
        .execution_control
        .as_ref()
        .map(CliExecutionControl::verbose)
        .unwrap_or(false);
    let mut result = await_with_heartbeat(
        &item.name,
        verbose,
        process(item.payload.clone(), resources.execution_control.clone()),
    )
    .await;
    if let Err(ref error) = result {
        if is_rate_limit_error(error)
            && prepare_rate_limit_retry(
                error,
                &item.name,
                item.estimated,
                resources.token_limiter.as_ref(),
                resources.rate_limiter.as_ref(),
            )
            .await
        {
            acquire_request_capacity(
                resources.token_limiter.as_ref(),
                resources.rate_limiter.as_ref(),
                item.estimated,
            )
            .await?;
            result = await_with_heartbeat(
                &item.name,
                verbose,
                process(item.payload, resources.execution_control.clone()),
            )
            .await;
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::parse_server_retry_delay;
    use anyhow::anyhow;
    use tokio::time::Duration;

    #[test]
    fn parses_retry_after_seconds_header_hint() {
        let error = anyhow!("HTTP 429 ... [rate-limit headers: Retry-After: 12]");
        assert_eq!(
            parse_server_retry_delay(&error),
            Some(Duration::from_secs(12))
        );
    }

    #[test]
    fn parses_millisecond_reset_hint() {
        let error = anyhow!("HTTP 429 ... [rate-limit headers: x-ratelimit-reset-requests: 250ms]");
        assert_eq!(
            parse_server_retry_delay(&error),
            Some(Duration::from_secs(1))
        );
    }

    #[test]
    fn parses_try_again_in_phrase() {
        let error = anyhow!("rate_limit_error: Please try again in 3.5s");
        assert_eq!(
            parse_server_retry_delay(&error),
            Some(Duration::from_secs_f64(3.5))
        );
    }
}

pub(crate) async fn run_stage_items<T, R, F, Fut>(
    items: Vec<StageItem<T>>,
    can_parallel: bool,
    progress: &mut ProgressIndicator,
    resources: &ExecutionResources,
    config: &Config,
    process: F,
) -> Result<Vec<(String, Result<R>)>>
where
    T: Clone + Send + 'static,
    R: Send + 'static,
    F: Fn(T, Option<CliExecutionControl>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<R>> + Send + 'static,
{
    let process = Arc::new(process);
    if can_parallel {
        let mut tasks = Vec::new();
        for item in items {
            if item.cache_hit {
                progress.start_item_cached(&item.name);
            } else {
                progress.start_item(&item.name, Some(item.estimated));
            }
            let name = item.name.clone();
            let resources = resources.clone();
            let process = process.clone();
            tasks.push(tokio::task::spawn(async move {
                let result = run_stage_item(item, resources, process).await;
                (name, result)
            }));
        }

        let mut results = Vec::new();
        for task in tasks {
            results.push(task.await?);
        }
        Ok(results)
    } else {
        let mut results = Vec::new();
        for item in items {
            if item.cache_hit {
                progress.start_item_cached(&item.name);
            } else {
                progress.start_item(&item.name, Some(item.estimated));
                if config.verbose {
                    println!("Processing context: {}", item.name);
                }
            }
            let name = item.name.clone();
            let result = run_stage_item(item, resources.clone(), process.clone()).await;
            results.push((name, result));
        }
        Ok(results)
    }
}
