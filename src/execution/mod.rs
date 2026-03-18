mod agent_input;
#[path = "../contexts/agent_runner.rs"]
mod agent_runner;
#[path = "../data/cache.rs"]
mod cache;
#[path = "../contexts/file_cache.rs"]
mod file_cache;
#[path = "../contexts/native_runner.rs"]
mod native_runner;
#[path = "../cli/token_limiter.rs"]
mod token_limiter;

pub use agent_input::{build_agent_input, output_contains_questions, AgentInput};
pub use agent_runner::{
    AgentModelRegistry, AgentRegistry, AgentRunner, AgentRunnerError, AgentSpecification,
    AgentSpecificationTemplate, ExecutionError, ExecutionResult, Model, PopulateError,
    PreparedExecution, PreparedExecutionState,
};
pub use cache::Cache;
pub use file_cache::FileCache;
pub use native_runner::{
    execute_request as execute_native_request,
    execute_request_with_metadata as execute_native_request_with_metadata, NativeExecutionControl,
    NativeExecutionMetadata, NativeExecutionResult, NativeRequestStep, NativeStepUsage,
};
pub use token_limiter::{
    estimate_request_tokens, estimate_tokens, TokenLimiter, CHARS_PER_TOKEN,
    REQUEST_OVERHEAD_TOKENS, TOKENS_PER_WORD,
};
