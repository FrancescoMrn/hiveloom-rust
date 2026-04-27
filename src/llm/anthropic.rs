use crate::llm::provider::{
    LlmProvider, LlmResponse, Message, TokenUsage, ToolCall, ToolDefinition,
};
use serde_json::json;
use tiktoken_rs::cl100k_base;

pub struct AnthropicProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
}

const API_URL: &str = "https://api.anthropic.com/v1/messages";
const ANTHROPIC_VERSION: &str = "2023-06-01";

impl AnthropicProvider {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
            model,
        }
    }

    /// Convert our generic messages into the Anthropic API format.
    ///
    /// The system prompt is extracted and returned separately; the remaining
    /// messages are returned as the `messages` array. Tool round-trips are
    /// emitted as native `tool_use` / `tool_result` content blocks so the
    /// model can thread them rather than treating them as plain text.
    fn build_request_body(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
    ) -> serde_json::Value {
        let mut system_parts: Vec<String> = Vec::new();
        let mut api_messages: Vec<serde_json::Value> = Vec::new();

        for msg in messages {
            // Anthropic expects the system prompt as a top-level field.
            if msg.role == "system" {
                system_parts.push(msg.content.clone());
                continue;
            }

            // Tool result: emit a user message with a tool_result content block.
            if let Some(tr) = &msg.tool_result {
                api_messages.push(json!({
                    "role": "user",
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": tr.tool_use_id,
                        "content": tr.content,
                    }],
                }));
                continue;
            }

            // Assistant turn carrying tool_use blocks (with optional accompanying text).
            if msg.role == "assistant" && !msg.tool_calls.is_empty() {
                let mut blocks: Vec<serde_json::Value> = Vec::new();
                if !msg.content.is_empty() {
                    blocks.push(json!({"type": "text", "text": msg.content}));
                }
                for tc in &msg.tool_calls {
                    blocks.push(json!({
                        "type": "tool_use",
                        "id": tc.id,
                        "name": tc.name,
                        "input": tc.arguments,
                    }));
                }
                api_messages.push(json!({
                    "role": "assistant",
                    "content": blocks,
                }));
                continue;
            }

            match msg.role.as_str() {
                "user" | "assistant" => {
                    api_messages.push(json!({
                        "role": msg.role,
                        "content": msg.content,
                    }));
                }
                other => {
                    api_messages.push(json!({
                        "role": "user",
                        "content": format!("[{}]\n{}", other, msg.content),
                    }));
                }
            }
        }

        let mut body = json!({
            "model": self.model,
            "max_tokens": 4096,
            "messages": api_messages,
        });

        if !system_parts.is_empty() {
            body["system"] = json!(system_parts.join("\n\n"));
        }

        if !tools.is_empty() {
            let tools_json: Vec<serde_json::Value> = tools
                .iter()
                .map(|t| {
                    json!({
                        "name": t.name,
                        "description": t.description,
                        "input_schema": t.input_schema,
                    })
                })
                .collect();
            body["tools"] = json!(tools_json);
        }

        body
    }
}

#[async_trait::async_trait]
impl LlmProvider for AnthropicProvider {
    async fn chat_complete(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
    ) -> anyhow::Result<LlmResponse> {
        let body = self.build_request_body(messages, tools);

        let resp = self
            .client
            .post(API_URL)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        let resp_body: serde_json::Value = resp.json().await?;

        if !status.is_success() {
            let err_msg = resp_body["error"]["message"]
                .as_str()
                .unwrap_or("unknown error");
            anyhow::bail!("Anthropic API error ({}): {}", status, err_msg);
        }

        // Parse content blocks
        let mut text_parts: Vec<String> = Vec::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();

        if let Some(content) = resp_body["content"].as_array() {
            for block in content {
                match block["type"].as_str() {
                    Some("text") => {
                        if let Some(text) = block["text"].as_str() {
                            text_parts.push(text.to_string());
                        }
                    }
                    Some("tool_use") => {
                        tool_calls.push(ToolCall {
                            id: block["id"].as_str().unwrap_or_default().to_string(),
                            name: block["name"].as_str().unwrap_or_default().to_string(),
                            arguments: block["input"].clone(),
                        });
                    }
                    _ => {}
                }
            }
        }

        let content = if text_parts.is_empty() {
            None
        } else {
            Some(text_parts.join(""))
        };

        let usage = TokenUsage {
            input_tokens: resp_body["usage"]["input_tokens"].as_u64().unwrap_or(0) as usize,
            output_tokens: resp_body["usage"]["output_tokens"].as_u64().unwrap_or(0) as usize,
        };

        Ok(LlmResponse {
            content,
            tool_calls,
            usage,
        })
    }

    fn count_tokens(&self, text: &str) -> usize {
        let bpe = cl100k_base().expect("failed to load cl100k_base tokenizer");
        bpe.encode_with_special_tokens(text).len()
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combines_multiple_system_messages() {
        let provider = AnthropicProvider::new("test-key".to_string(), "claude-test".to_string());
        let body = provider.build_request_body(
            &[
                Message::text("system", "base prompt"),
                Message::text("system", "compaction summary"),
                Message::text("user", "hello"),
            ],
            &[],
        );

        let system = body["system"].as_str().unwrap();
        assert!(system.contains("base prompt"));
        assert!(system.contains("compaction summary"));
    }

    #[test]
    fn maps_unknown_roles_to_user_messages() {
        let provider = AnthropicProvider::new("test-key".to_string(), "claude-test".to_string());
        let body =
            provider.build_request_body(&[Message::text("tool_result", "{\"ok\":true}")], &[]);

        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(
            body["messages"][0]["content"],
            "[tool_result]\n{\"ok\":true}"
        );
    }

    #[test]
    fn emits_tool_use_and_tool_result_content_blocks() {
        let provider = AnthropicProvider::new("test-key".to_string(), "claude-test".to_string());
        let tool_call = ToolCall {
            id: "toolu_01abc".to_string(),
            name: "lookup".to_string(),
            arguments: json!({"q": "ping"}),
        };
        let body = provider.build_request_body(
            &[
                Message::text("user", "look it up"),
                Message::assistant_with_tools("on it", vec![tool_call.clone()]),
                Message::tool_result(&tool_call.id, "{\"answer\":\"pong\"}"),
            ],
            &[],
        );

        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 3);

        // Plain user message keeps its string content.
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[0]["content"], "look it up");

        // Assistant turn becomes structured blocks: text + tool_use.
        assert_eq!(messages[1]["role"], "assistant");
        let blocks = messages[1]["content"].as_array().unwrap();
        assert_eq!(blocks[0]["type"], "text");
        assert_eq!(blocks[0]["text"], "on it");
        assert_eq!(blocks[1]["type"], "tool_use");
        assert_eq!(blocks[1]["id"], "toolu_01abc");
        assert_eq!(blocks[1]["name"], "lookup");
        assert_eq!(blocks[1]["input"]["q"], "ping");

        // Tool result becomes a user message with a single tool_result block.
        assert_eq!(messages[2]["role"], "user");
        let result_blocks = messages[2]["content"].as_array().unwrap();
        assert_eq!(result_blocks[0]["type"], "tool_result");
        assert_eq!(result_blocks[0]["tool_use_id"], "toolu_01abc");
        assert_eq!(result_blocks[0]["content"], "{\"answer\":\"pong\"}");
    }

    #[test]
    fn assistant_tool_use_without_text_omits_the_text_block() {
        let provider = AnthropicProvider::new("test-key".to_string(), "claude-test".to_string());
        let body = provider.build_request_body(
            &[Message::assistant_with_tools(
                "",
                vec![ToolCall {
                    id: "toolu_silent".to_string(),
                    name: "ping".to_string(),
                    arguments: json!({}),
                }],
            )],
            &[],
        );

        let blocks = body["messages"][0]["content"].as_array().unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0]["type"], "tool_use");
    }
}
