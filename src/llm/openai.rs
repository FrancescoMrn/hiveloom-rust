use crate::llm::provider::{
    LlmProvider, LlmResponse, Message, TokenUsage, ToolCall, ToolDefinition,
};
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
        let base_url = base_url
            .map(|url| url.trim().trim_end_matches('/').to_string())
            .filter(|url| !url.is_empty())
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
        Self {
            client: reqwest::Client::new(),
            api_key,
            model,
            base_url,
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
                // Tool result: must be `role: tool` with `tool_call_id`.
                if let Some(tr) = &m.tool_result {
                    return json!({
                        "role": "tool",
                        "tool_call_id": tr.tool_use_id,
                        "content": tr.content,
                    });
                }
                // Assistant turn carrying tool calls: emit a `tool_calls`
                // array; OpenAI requires arguments as a JSON-encoded string.
                if m.role == "assistant" && !m.tool_calls.is_empty() {
                    let calls: Vec<serde_json::Value> = m
                        .tool_calls
                        .iter()
                        .map(|tc| {
                            json!({
                                "id": tc.id,
                                "type": "function",
                                "function": {
                                    "name": tc.name,
                                    "arguments": serde_json::to_string(&tc.arguments)
                                        .unwrap_or_else(|_| "{}".to_string()),
                                },
                            })
                        })
                        .collect();
                    let content = if m.content.is_empty() {
                        serde_json::Value::Null
                    } else {
                        serde_json::Value::String(m.content.clone())
                    };
                    return json!({
                        "role": "assistant",
                        "content": content,
                        "tool_calls": calls,
                    });
                }
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
                    name: function["name"].as_str().unwrap_or_default().to_string(),
                    arguments,
                });
            }
        }

        let usage = TokenUsage {
            input_tokens: resp_body["usage"]["prompt_tokens"].as_u64().unwrap_or(0) as usize,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_custom_base_url_or_uses_default() {
        let default_provider = OpenAiProvider::new("key".into(), "gpt-test".into(), None);
        assert_eq!(default_provider.base_url, DEFAULT_BASE_URL);

        let custom_provider = OpenAiProvider::new(
            "key".into(),
            "local-model".into(),
            Some(" https://llm.example.test/v1/ ".into()),
        );
        assert_eq!(custom_provider.base_url, "https://llm.example.test/v1");
    }

    #[test]
    fn emits_tool_calls_array_and_role_tool_messages() {
        let provider = OpenAiProvider::new("key".into(), "gpt-test".into(), None);
        let tool_call = ToolCall {
            id: "call_42".to_string(),
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

        // Plain user message untouched.
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[0]["content"], "look it up");

        // Assistant turn carries `tool_calls` with arguments encoded as JSON
        // string (OpenAI quirk) and content present alongside.
        assert_eq!(messages[1]["role"], "assistant");
        assert_eq!(messages[1]["content"], "on it");
        let calls = messages[1]["tool_calls"].as_array().unwrap();
        assert_eq!(calls[0]["id"], "call_42");
        assert_eq!(calls[0]["type"], "function");
        assert_eq!(calls[0]["function"]["name"], "lookup");
        let raw_args = calls[0]["function"]["arguments"].as_str().unwrap();
        let parsed_args: serde_json::Value = serde_json::from_str(raw_args).unwrap();
        assert_eq!(parsed_args["q"], "ping");

        // Tool result becomes role: tool with the matching tool_call_id.
        assert_eq!(messages[2]["role"], "tool");
        assert_eq!(messages[2]["tool_call_id"], "call_42");
        assert_eq!(messages[2]["content"], "{\"answer\":\"pong\"}");
    }

    #[test]
    fn assistant_tool_use_without_text_sends_null_content() {
        let provider = OpenAiProvider::new("key".into(), "gpt-test".into(), None);
        let body = provider.build_request_body(
            &[Message::assistant_with_tools(
                "",
                vec![ToolCall {
                    id: "call_silent".to_string(),
                    name: "ping".to_string(),
                    arguments: json!({}),
                }],
            )],
            &[],
        );

        assert!(body["messages"][0]["content"].is_null());
        assert!(body["messages"][0]["tool_calls"].is_array());
    }
}
