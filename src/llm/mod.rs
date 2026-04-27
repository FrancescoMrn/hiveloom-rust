pub mod anthropic;
pub mod openai;
pub mod provider;

pub use anthropic::AnthropicProvider;
pub use openai::OpenAiProvider;
pub use provider::{
    LlmProvider, LlmResponse, Message, TokenUsage, ToolCall, ToolDefinition, ToolResult,
};

/// Resolve a provider implementation from a model identifier and API key.
///
/// - Model IDs starting with `claude-` use the Anthropic Messages API.
/// - Model IDs starting with `gpt-`, `o1-`, or `o3-` use the OpenAI Chat
///   Completions API.
/// - All other model IDs fall through to OpenAI-compatible mode, which works
///   for APIs that implement the OpenAI chat completions contract.
/// - `HIVELOOM_OPENAI_BASE_URL` overrides the OpenAI-compatible endpoint.
pub fn resolve_provider(
    model_id: &str,
    api_key: &str,
) -> anyhow::Result<Box<dyn provider::LlmProvider>> {
    if model_id.starts_with("claude-") {
        Ok(Box::new(AnthropicProvider::new(
            api_key.to_string(),
            model_id.to_string(),
        )))
    } else {
        // gpt-*, o1-*, o3-*, or any OpenAI-compatible model
        let base_url = std::env::var("HIVELOOM_OPENAI_BASE_URL")
            .ok()
            .or_else(|| std::env::var("HIVELOOM_OPENAI_COMPAT_BASE_URL").ok());
        Ok(Box::new(OpenAiProvider::new(
            api_key.to_string(),
            model_id.to_string(),
            base_url,
        )))
    }
}
