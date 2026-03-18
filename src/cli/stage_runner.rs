use anyhow::Result;
use std::future::Future;
use std::sync::Arc;
use tokio::task::block_in_place;
use tokio::time::sleep;

use reen::execution::{NativeExecutionControl, NativeRequestStep, NativeStepUsage, TokenLimiter};

use super::agent_executor::AgentExecutor;
use super::progress::ProgressIndicator;
use super::rate_limiter::RateLimiter;
use super::Config;

#[derive(Clone)]
pub(crate) struct CliExecutionControl {
    token_limiter: Option<Arc<TokenLimiter>>,
    rate_limiter: Option<Arc<RateLimiter>>,
}

impl CliExecutionControl {
    pub(crate) fn new(
        token_limiter: Option<Arc<TokenLimiter>>,
        rate_limiter: Option<Arc<RateLimiter>>,
    ) -> Self {
        Self {
            token_limiter,
            rate_limiter,
        }
    }
}

impl NativeExecutionControl for CliExecutionControl {
    fn before_model_request(&self, step: &NativeRequestStep) -> Result<(), String> {
        if let Some(limiter) = &self.token_limiter {
            let exceeds_limit =
                block_in_place(|| limiter.exceeds_limit_blocking(step.estimated_input_tokens));
            if exceeds_limit {
                return Err(format!(
                    "Estimated request size ({} input tokens) exceeds configured --token-limit/REEN_TOKEN_LIMIT budget for a single minute.",
                    step.estimated_input_tokens
                ));
            }
            block_in_place(|| limiter.acquire_tokens_blocking(step.estimated_input_tokens));
        }
        if let Some(limiter) = &self.rate_limiter {
            block_in_place(|| limiter.acquire_blocking());
        }
        Ok(())
    }

    fn after_model_response(&self, usage: &NativeStepUsage) {
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
    pub(crate) fn new(rate_limit: Option<f64>, token_limit: Option<f64>) -> Self {
        let rate_limiter = rate_limit.map(RateLimiter::new).map(Arc::new);
        let token_limiter = token_limit.map(TokenLimiter::new).map(Arc::new);
        let execution_control = Some(CliExecutionControl::new(
            token_limiter.clone(),
            rate_limiter.clone(),
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

pub(crate) async fn acquire_request_capacity(
    token_limiter: Option<&Arc<TokenLimiter>>,
    rate_limiter: Option<&Arc<RateLimiter>>,
    estimated: usize,
) -> Result<()> {
    if let Some(limiter) = token_limiter {
        if limiter.exceeds_limit(estimated).await {
            anyhow::bail!(
                "Estimated request size ({estimated} input tokens) exceeds configured --token-limit/REEN_TOKEN_LIMIT budget for a single minute. Reduce prompt size or raise the token limit."
            );
        }
    }
    if let Some(limiter) = rate_limiter {
        let _ = limiter;
    }
    Ok(())
}

pub(crate) async fn prepare_rate_limit_retry(
    item_name: &str,
    estimated: usize,
    token_limiter: Option<&Arc<TokenLimiter>>,
    rate_limiter: Option<&Arc<RateLimiter>>,
) -> bool {
    let mut waited = false;
    if let Some(limiter) = token_limiter {
        let delay = limiter.retry_delay(estimated).await;
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
            eprintln!(
                "Rate limit (429) exceeded for {}, waiting and retrying with slower rate...",
                item_name
            );
            sleep(limiter.retry_delay()).await;
        }
        limiter.back_off().await;
        waited = true;
    }
    waited
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
    Fut: Future<Output = Result<R>> + Send,
{
    if !item.cache_hit {
        acquire_request_capacity(
            resources.token_limiter.as_ref(),
            resources.rate_limiter.as_ref(),
            item.estimated,
        )
        .await?;
    }

    let mut result = process(item.payload.clone(), resources.execution_control.clone()).await;
    if let Err(ref error) = result {
        if is_rate_limit_error(error)
            && prepare_rate_limit_retry(
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
            result = process(item.payload, resources.execution_control.clone()).await;
        }
    }

    result
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
