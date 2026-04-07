use chrono::Utc;
use reen::execution::NativeExecutionMetadata;
use serde::Serialize;
use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use super::progress::{OutputTone, header_text, muted_text, paint_stdout, warning_tag};

#[derive(Clone, Debug)]
pub(crate) struct UsageScope {
    pub(crate) stage: String,
    pub(crate) artifact_name: String,
    pub(crate) artifact_path: Option<String>,
    pub(crate) estimated_input_tokens: Option<usize>,
}

impl UsageScope {
    pub(crate) fn new(stage: impl Into<String>, artifact_name: impl Into<String>) -> Self {
        Self {
            stage: stage.into(),
            artifact_name: artifact_name.into(),
            artifact_path: None,
            estimated_input_tokens: None,
        }
    }

    pub(crate) fn with_path(mut self, artifact_path: impl Into<String>) -> Self {
        self.artifact_path = Some(artifact_path.into());
        self
    }

    pub(crate) fn with_estimated_input_tokens(mut self, estimated_input_tokens: usize) -> Self {
        self.estimated_input_tokens = Some(estimated_input_tokens);
        self
    }
}

#[derive(Clone)]
pub(crate) struct UsageReporter {
    inner: Arc<UsageReporterInner>,
}

struct UsageReporterInner {
    command_name: String,
    project_root: PathBuf,
    verbose: bool,
    state: Mutex<UsageReporterState>,
}

#[derive(Default)]
struct UsageReporterState {
    events: Vec<AgentUsageEvent>,
    emitted: bool,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct AgentUsageEvent {
    pub(crate) recorded_at: String,
    pub(crate) stage: String,
    pub(crate) artifact_name: String,
    pub(crate) artifact_path: Option<String>,
    pub(crate) agent_name: String,
    pub(crate) provider: String,
    pub(crate) model: String,
    pub(crate) cache_hit: bool,
    pub(crate) elapsed_ms: u128,
    pub(crate) estimated_input_tokens: usize,
    pub(crate) input_tokens: Option<u64>,
    pub(crate) output_tokens: Option<u64>,
    pub(crate) total_tokens: Option<u64>,
    pub(crate) request_steps: usize,
}

#[derive(Debug, Clone, Serialize)]
struct StageAgentUsageSummary {
    stage: String,
    agent_name: String,
    calls: usize,
    cached_calls: usize,
    elapsed_ms: u128,
    estimated_input_tokens: usize,
    input_tokens: u64,
    output_tokens: u64,
    total_tokens: u64,
}

#[derive(Debug, Clone, Serialize)]
struct UsageSummaryDocument {
    command_name: String,
    generated_at: String,
    total_calls: usize,
    cached_calls: usize,
    elapsed_ms: u128,
    estimated_input_tokens: usize,
    input_tokens: u64,
    output_tokens: u64,
    total_tokens: u64,
    stage_agent_summaries: Vec<StageAgentUsageSummary>,
    slowest_calls: Vec<AgentUsageEvent>,
    highest_token_calls: Vec<AgentUsageEvent>,
    events: Vec<AgentUsageEvent>,
}

impl UsageReporter {
    pub(crate) fn new(
        command_name: impl Into<String>,
        project_root: impl Into<PathBuf>,
        verbose: bool,
    ) -> Self {
        Self {
            inner: Arc::new(UsageReporterInner {
                command_name: sanitize_name(&command_name.into()),
                project_root: project_root.into(),
                verbose,
                state: Mutex::new(UsageReporterState::default()),
            }),
        }
    }

    pub(crate) fn record(
        &self,
        scope: &UsageScope,
        agent_name: &str,
        provider: &str,
        model: &str,
        cache_hit: bool,
        elapsed_ms: u128,
        usage: Option<&NativeExecutionMetadata>,
    ) {
        let (estimated_input_tokens, input_tokens, output_tokens, total_tokens, request_steps) =
            fold_usage(scope.estimated_input_tokens, usage);
        let event = AgentUsageEvent {
            recorded_at: Utc::now().to_rfc3339(),
            stage: scope.stage.clone(),
            artifact_name: scope.artifact_name.clone(),
            artifact_path: scope.artifact_path.clone(),
            agent_name: agent_name.to_string(),
            provider: provider.to_string(),
            model: model.to_string(),
            cache_hit,
            elapsed_ms,
            estimated_input_tokens,
            input_tokens,
            output_tokens,
            total_tokens,
            request_steps,
        };

        if let Ok(mut state) = self.inner.state.lock() {
            state.events.push(event);
        }
    }

    pub(crate) fn emit_summary_if_needed(&self) -> std::io::Result<Option<PathBuf>> {
        let mut state = self
            .inner
            .state
            .lock()
            .expect("usage reporter mutex should not be poisoned");
        if state.emitted {
            return Ok(None);
        }
        state.emitted = true;

        let summary = build_summary(&self.inner.command_name, &state.events);
        let path = usage_report_path(
            &self.inner.project_root,
            &self.inner.command_name,
            &summary.generated_at,
        );
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, serde_json::to_string_pretty(&summary).unwrap_or_else(|_| "{}".into()))?;
        if !summary.events.is_empty() || self.inner.verbose {
            print_summary(&summary, &path);
        }
        Ok(Some(path))
    }
}

impl Drop for UsageReporter {
    fn drop(&mut self) {
        if Arc::strong_count(&self.inner) != 1 {
            return;
        }
        if let Err(error) = self.emit_summary_if_needed() {
            eprintln!("{} failed to write usage summary: {error}", warning_tag("usage"));
        }
    }
}

fn fold_usage(
    fallback_estimated_input_tokens: Option<usize>,
    usage: Option<&NativeExecutionMetadata>,
) -> (usize, Option<u64>, Option<u64>, Option<u64>, usize) {
    let Some(usage) = usage else {
        return (fallback_estimated_input_tokens.unwrap_or(0), None, None, None, 0);
    };

    let estimated_input_tokens = usage
        .steps
        .iter()
        .map(|step| step.estimated_input_tokens)
        .sum::<usize>()
        .max(fallback_estimated_input_tokens.unwrap_or(0));
    let input_tokens = sum_optional(usage.steps.iter().map(|step| step.input_tokens));
    let output_tokens = sum_optional(usage.steps.iter().map(|step| step.output_tokens));
    let total_tokens = sum_optional(usage.steps.iter().map(|step| step.total_tokens));

    (
        estimated_input_tokens,
        input_tokens,
        output_tokens,
        total_tokens,
        usage.steps.len(),
    )
}

fn sum_optional<I>(values: I) -> Option<u64>
where
    I: IntoIterator<Item = Option<u64>>,
{
    let mut saw_value = false;
    let mut total = 0u64;
    for value in values {
        if let Some(value) = value {
            saw_value = true;
            total = total.saturating_add(value);
        }
    }
    saw_value.then_some(total)
}

fn build_summary(command_name: &str, events: &[AgentUsageEvent]) -> UsageSummaryDocument {
    let mut stage_agent_map: BTreeMap<(String, String), StageAgentUsageSummary> = BTreeMap::new();
    let mut cached_calls = 0usize;
    let mut elapsed_ms = 0u128;
    let mut estimated_input_tokens = 0usize;
    let mut input_tokens = 0u64;
    let mut output_tokens = 0u64;
    let mut total_tokens = 0u64;

    for event in events {
        if event.cache_hit {
            cached_calls += 1;
        }
        elapsed_ms = elapsed_ms.saturating_add(event.elapsed_ms);
        estimated_input_tokens =
            estimated_input_tokens.saturating_add(event.estimated_input_tokens);
        input_tokens = input_tokens.saturating_add(event.input_tokens.unwrap_or(0));
        output_tokens = output_tokens.saturating_add(event.output_tokens.unwrap_or(0));
        total_tokens = total_tokens.saturating_add(event.total_tokens.unwrap_or(0));

        let entry = stage_agent_map
            .entry((event.stage.clone(), event.agent_name.clone()))
            .or_insert_with(|| StageAgentUsageSummary {
                stage: event.stage.clone(),
                agent_name: event.agent_name.clone(),
                calls: 0,
                cached_calls: 0,
                elapsed_ms: 0,
                estimated_input_tokens: 0,
                input_tokens: 0,
                output_tokens: 0,
                total_tokens: 0,
            });
        entry.calls += 1;
        entry.cached_calls += usize::from(event.cache_hit);
        entry.elapsed_ms = entry.elapsed_ms.saturating_add(event.elapsed_ms);
        entry.estimated_input_tokens = entry
            .estimated_input_tokens
            .saturating_add(event.estimated_input_tokens);
        entry.input_tokens = entry.input_tokens.saturating_add(event.input_tokens.unwrap_or(0));
        entry.output_tokens = entry
            .output_tokens
            .saturating_add(event.output_tokens.unwrap_or(0));
        entry.total_tokens = entry.total_tokens.saturating_add(event.total_tokens.unwrap_or(0));
    }

    let mut slowest_calls = events.to_vec();
    slowest_calls.sort_by_key(|event| Reverse(event.elapsed_ms));
    slowest_calls.truncate(5);

    let mut highest_token_calls = events.to_vec();
    highest_token_calls.sort_by_key(|event| {
        Reverse(
            event
                .total_tokens
                .unwrap_or(event.input_tokens.unwrap_or(event.estimated_input_tokens as u64)),
        )
    });
    highest_token_calls.truncate(5);

    UsageSummaryDocument {
        command_name: command_name.to_string(),
        generated_at: Utc::now().format("%Y%m%dT%H%M%SZ").to_string(),
        total_calls: events.len(),
        cached_calls,
        elapsed_ms,
        estimated_input_tokens,
        input_tokens,
        output_tokens,
        total_tokens,
        stage_agent_summaries: stage_agent_map.into_values().collect(),
        slowest_calls,
        highest_token_calls,
        events: events.to_vec(),
    }
}

fn usage_report_path(project_root: &Path, command_name: &str, generated_at: &str) -> PathBuf {
    project_root
        .join(".reen")
        .join("pipeline_quality")
        .join("usage")
        .join(format!("{command_name}_{generated_at}.json"))
}

fn print_summary(summary: &UsageSummaryDocument, report_path: &Path) {
    println!("\n{}", paint_stdout("=".repeat(60), OutputTone::Muted));
    println!("{}", header_text("Usage Summary:"));
    println!(
        "  {} {} {}",
        muted_text("Calls:"),
        summary.total_calls,
        muted_text(format!("(cached {})", summary.cached_calls))
    );
    println!(
        "  {} {:.2}s",
        muted_text("Duration:"),
        summary.elapsed_ms as f64 / 1000.0
    );
    println!("  {} {}", muted_text("Est input:"), summary.estimated_input_tokens);
    println!(
        "  {} input {} / output {} / total {}",
        muted_text("Tokens:"),
        summary.input_tokens, summary.output_tokens, summary.total_tokens
    );

    for entry in summary.stage_agent_summaries.iter().take(8) {
        println!(
            "  {} / {}: {} call(s), {} cached, {} total tokens, {:.2}s",
            paint_stdout(&entry.stage, OutputTone::Progress),
            paint_stdout(&entry.agent_name, OutputTone::Standard),
            entry.calls,
            entry.cached_calls,
            entry.total_tokens,
            entry.elapsed_ms as f64 / 1000.0
        );
    }

    if let Some(slowest) = summary.slowest_calls.first() {
        println!(
            "  Slowest:   {} / {} / {} ({:.2}s)",
            paint_stdout(&slowest.stage, OutputTone::Progress),
            paint_stdout(&slowest.agent_name, OutputTone::Standard),
            slowest.artifact_name,
            slowest.elapsed_ms as f64 / 1000.0
        );
    }
    if let Some(highest) = summary.highest_token_calls.first() {
        println!(
            "  Highest:   {} / {} / {} ({} total tokens)",
            paint_stdout(&highest.stage, OutputTone::Progress),
            paint_stdout(&highest.agent_name, OutputTone::Standard),
            highest.artifact_name,
            highest.total_tokens.unwrap_or(0)
        );
    }
    println!("  {} {}", muted_text("Report:"), report_path.display());
    println!("{}", paint_stdout("=".repeat(60), OutputTone::Muted));
}

fn sanitize_name(value: &str) -> String {
    let mut out = String::new();
    let mut last_separator = false;
    for ch in value.chars() {
        let normalized = ch.to_ascii_lowercase();
        if normalized.is_ascii_alphanumeric() {
            out.push(normalized);
            last_separator = false;
        } else if !last_separator {
            out.push('_');
            last_separator = true;
        }
    }
    out.trim_matches('_').to_string()
}

#[cfg(test)]
mod tests {
    use super::{UsageReporter, UsageScope};
    use reen::execution::{NativeExecutionMetadata, NativeStepUsage};

    #[test]
    fn usage_reporter_writes_summary() {
        let root = std::env::temp_dir().join(format!("reen_usage_report_{}", std::process::id()));
        let reporter = UsageReporter::new("create specification", &root, false);
        let scope = UsageScope::new("specification", "terminal_renderer")
            .with_path("drafts/contexts/terminal_renderer.md")
            .with_estimated_input_tokens(1200);
        reporter.record(
            &scope,
            "create_specifications_context",
            "openai",
            "gpt-5",
            false,
            250,
            Some(&NativeExecutionMetadata {
                steps: vec![NativeStepUsage {
                    provider: "openai".to_string(),
                    model: "gpt-5".to_string(),
                    estimated_input_tokens: 1200,
                    input_tokens: Some(1180),
                    output_tokens: Some(300),
                    total_tokens: Some(1480),
                }],
            }),
        );

        let path = reporter
            .emit_summary_if_needed()
            .expect("emit summary")
            .expect("report path");
        let content = std::fs::read_to_string(&path).expect("read usage report");
        assert!(content.contains("\"command_name\": \"create_specification\""));
        assert!(content.contains("\"agent_name\": \"create_specifications_context\""));

        std::fs::remove_dir_all(root).ok();
    }
}
