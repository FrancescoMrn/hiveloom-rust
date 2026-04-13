use crate::llm::provider::{LlmProvider, LlmResponse, Message, TokenUsage, ToolCall, ToolDefinition};
use serde_json::json;
use tiktoken_rs::cl100k_base;

const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";

pub struct OpenAiProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
    base_url: String,
}

impl OpenAiProvider {
    pub fn new(api_key: String, model: String, base_url: Option<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
            model,
            base_url: base_url.unwrap_or_else(|| DEFAULT_BASE_URL.to_string()),
        }
    }

    fn build_request_body(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
    ) -> serde_json::Value {
        let api_messages: Vec<serde_json::Value> = messages
            .iter()
            .map(|m| {
                json!({
                    "role": m.role,
                    "content": m.content,
                })
            })
            .collect();

        let mut body = json!({
            "model": self.model,
            "messages": api_messages,
        });

        if !tools.is_empty() {
            let tools_json: Vec<serde_json::Value> = tools
                .iter()
                .map(|t| {
                    json!({
                        "type": "function",
                        "function": {
                            "name": t.name,
                            "description": t.description,
                            "parameters": t.input_schema,
                        }
                    })
                })
                .collect();
            body["tools"] = json!(tools_json);
        }

        body
    }
}

#[async_trait::async_trait]
impl LlmProvider for OpenAiProvider {
    async fn chat_complete(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
    ) -> anyhow::Result<LlmResponse> {
        let url = format!("{}/chat/completions", self.base_url);
        let body = self.build_request_body(messages, tools);

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        let resp_body: serde_json::Value = resp.json().await?;

        if !status.is_success() {
            let err_msg = resp_body["error"]["message"]
                .as_str()
                .unwrap_or("unknown error");
            anyhow::bail!("OpenAI API error ({}): {}", status, err_msg);
        }

        let choice = &resp_body["choices"][0];
        let message = &choice["message"];

        // Extract text content
        let content = message["content"].as_str().map(|s| s.to_string());

        // Extract tool calls
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        if let Some(calls) = message["tool_calls"].as_array() {
            for call in calls {
                let function = &call["function"];
                let arguments_str = function["arguments"].as_str().unwrap_or("{}");
                let arguments: serde_json::Value =
                    serde_json::from_str(arguments_str).unwrap_or(json!({}));
                tool_calls.push(ToolCall {
                    id: call["id"].as_str().unwrap_or_default().to_string(),
                    name: function["name"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                    arguments,
                });
            }
        }

        let usage = TokenUsage {
            input_tokens: resp_body["usage"]["prompt_tokens"]
                .as_u64()
                .unwrap_or(0) as usize,
            output_tokens: resp_body["usage"]["completion_tokens"]
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
