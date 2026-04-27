use serde::{Deserialize, Serialize};

/// One conversation message handed to a provider.
///
/// Plain user/assistant/system text messages only need `role` + `content`.
/// Tool round-trips are threaded through the structured fields so providers
/// can serialise them as native content blocks (Anthropic) or
/// `tool_calls` / `role: tool` (OpenAI), keeping the model's view of the
/// conversation coherent across iterations.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    #[serde(default)]
    pub content: String,
    /// Tool calls the assistant emitted in this message. Empty for everything
    /// that isn't an assistant tool-use turn.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    /// Result of a tool call, paired with the assistant tool_use id. Set on
    /// the user/tool turn that follows a tool invocation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_result: Option<ToolResult>,
}

impl Message {
    /// Plain text message with no tool threading.
    pub fn text(role: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: content.into(),
            tool_calls: Vec::new(),
            tool_result: None,
        }
    }

    /// Assistant turn that invoked one or more tools. `content` is the
    /// optional accompanying text the model produced alongside the tool calls.
    pub fn assistant_with_tools(content: impl Into<String>, tool_calls: Vec<ToolCall>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: content.into(),
            tool_calls,
            tool_result: None,
        }
    }

    /// User/tool turn carrying the result of a single tool invocation.
    pub fn tool_result(tool_use_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: String::new(),
            tool_calls: Vec::new(),
            tool_result: Some(ToolResult {
                tool_use_id: tool_use_id.into(),
                content: content.into(),
            }),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_use_id: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct LlmResponse {
    pub content: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub usage: TokenUsage,
}

#[derive(Debug, Clone, Default)]
pub struct TokenUsage {
    pub input_tokens: usize,
    pub output_tokens: usize,
}

#[async_trait::async_trait]
pub trait LlmProvider: Send + Sync {
    /// Send a chat completion request with optional tool definitions.
    async fn chat_complete(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
    ) -> anyhow::Result<LlmResponse>;

    /// Estimate token count for the given text.
    fn count_tokens(&self, text: &str) -> usize;

    /// Return the model identifier string.
    fn model_name(&self) -> &str;
}
