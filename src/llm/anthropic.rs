use crate::llm::provider::{LlmProvider, LlmResponse, Message, TokenUsage, ToolCall, ToolDefinition};
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
    /// messages are returned as the `messages` array.
    fn build_request_body(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
    ) -> serde_json::Value {
        let mut system_prompt: Option<String> = None;
        let mut api_messages: Vec<serde_json::Value> = Vec::new();

        for msg in messages {
            if msg.role == "system" {
                // Anthropic expects the system prompt as a top-level field
                system_prompt = Some(msg.content.clone());
            } else {
                api_messages.push(json!({
                    "role": msg.role,
                    "content": msg.content,
                }));
            }
        }

        let mut body = json!({
            "model": self.model,
            "max_tokens": 4096,
            "messages": api_messages,
        });

        if let Some(sys) = system_prompt {
            body["system"] = json!(sys);
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
                            id: block["id"]
                                .as_str()
                                .unwrap_or_default()
                                .to_string(),
                            name: block["name"]
                                .as_str()
                                .unwrap_or_default()
                                .to_string(),
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
            input_tokens: resp_body["usage"]["input_tokens"]
                .as_u64()
                .unwrap_or(0) as usize,
            output_tokens: resp_body["usage"]["output_tokens"]
                .as_u64()
                .unwrap_or(0) as usize,
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
