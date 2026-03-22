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

pub use agent_input::{AgentInput, build_agent_input, output_contains_questions};
pub use agent_runner::{
    AgentModelRegistry, AgentRegistry, AgentRunner, AgentRunnerError, AgentSpecification,
    AgentSpecificationTemplate, ExecutionError, ExecutionResult, Model, PopulateError,
    PreparedExecution, PreparedExecutionState,
};
pub use cache::Cache;
pub use file_cache::FileCache;
pub use native_runner::{
    NativeExecutionControl, NativeExecutionMetadata, NativeExecutionResult, NativeRequestStep,
    NativeStepUsage, execute_request as execute_native_request,
    execute_request_with_metadata as execute_native_request_with_metadata,
};
pub use token_limiter::{
    CHARS_PER_TOKEN, REQUEST_OVERHEAD_TOKENS, TOKENS_PER_WORD, TokenLimiter,
    estimate_request_tokens, estimate_tokens,
};
