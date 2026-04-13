pub mod auth;
pub mod events;

use axum::{body::Bytes, extract::State, http::HeaderMap, Json};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::sync::Arc;

use crate::engine::chat_surface::{ChatSurface, SurfaceType};

type HmacSha256 = Hmac<Sha256>;

/// Verify the Slack request signature using HMAC-SHA256.
fn verify_signature(
    signing_secret: &str,
    timestamp: &str,
    body: &[u8],
    expected_sig: &str,
) -> bool {
    let base_string = format!("v0:{}:{}", timestamp, String::from_utf8_lossy(body));
    let mut mac = match HmacSha256::new_from_slice(signing_secret.as_bytes()) {
        Ok(m) => m,
        Err(_) => return false,
    };
    mac.update(base_string.as_bytes());
    let result = mac.finalize();
    let computed = format!("v0={}", hex::encode(result.into_bytes()));
    // Constant-time comparison via hmac verification
    computed == expected_sig
}

/// POST /slack/events — Slack event webhook handler.
///
/// Handles `url_verification` challenges and dispatches `event_callback` payloads
/// to the event dispatch module.
pub async fn handle_event(
    State(state): State<Arc<super::AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<serde_json::Value>, axum::http::StatusCode> {
    // Require Slack config
    let slack_config = state
        .slack_config
        .as_ref()
        .ok_or(axum::http::StatusCode::SERVICE_UNAVAILABLE)?;

    // Extract signature headers
    let timestamp = headers
        .get("x-slack-request-timestamp")
        .and_then(|v| v.to_str().ok())
        .ok_or(axum::http::StatusCode::BAD_REQUEST)?;

    let signature = headers
        .get("x-slack-signature")
        .and_then(|v| v.to_str().ok())
        .ok_or(axum::http::StatusCode::BAD_REQUEST)?;

    // Verify signature
    if !verify_signature(&slack_config.signing_secret, timestamp, &body, signature) {
        return Err(axum::http::StatusCode::UNAUTHORIZED);
    }

    // Parse JSON body
    let payload: serde_json::Value =
        serde_json::from_slice(&body).map_err(|_| axum::http::StatusCode::BAD_REQUEST)?;

    let event_type = payload
        .get("type")
        .and_then(|v| v.as_str())
        .ok_or(axum::http::StatusCode::BAD_REQUEST)?;

    match event_type {
        "url_verification" => {
            let challenge = payload
                .get("challenge")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            Ok(Json(serde_json::json!({ "challenge": challenge })))
        }
        "event_callback" => {
            // Parse into SlackEvent and dispatch
            let event = payload
                .get("event")
                .cloned()
                .ok_or(axum::http::StatusCode::BAD_REQUEST)?;

            let event_id = payload
                .get("event_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let slack_event = events::SlackEvent {
                event_id,
                event_type: event
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                channel: event
                    .get("channel")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                user: event
                    .get("user")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                text: event
                    .get("text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                thread_ts: event
                    .get("thread_ts")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                ts: event
                    .get("ts")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
            };

            let bot_token = slack_config.bot_token.clone();

            // Spawn dispatch in the background so we respond 200 quickly
            tokio::spawn(async move {
                if let Err(e) = events::dispatch_event(&state, &slack_event, &bot_token).await {
                    tracing::error!(error = %e, event_id = %slack_event.event_id, "Slack event dispatch failed");
                }
            });

            Ok(Json(serde_json::json!({ "ok": true })))
        }
        _ => {
            // Unknown event type — acknowledge silently
            Ok(Json(serde_json::json!({ "ok": true })))
        }
    }
}

// ── T040: SlackSurface ──────────────────────────────────────────────────

pub struct SlackSurface {
    bot_token: String,
    client: reqwest::Client,
}

impl SlackSurface {
    pub fn new(bot_token: &str) -> Self {
        Self {
            bot_token: bot_token.to_string(),
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait::async_trait]
impl ChatSurface for SlackSurface {
    async fn send_message(
        &self,
        surface_ref: &str,
        thread_ref: Option<&str>,
        content: &str,
    ) -> anyhow::Result<()> {
        let mut body = serde_json::json!({
            "channel": surface_ref,
            "text": content,
        });
        if let Some(ts) = thread_ref {
            body["thread_ts"] = serde_json::Value::String(ts.to_string());
        }

        let resp = self
            .client
            .post("https://slack.com/api/chat.postMessage")
            .header("Authorization", format!("Bearer {}", self.bot_token))
            .header("Content-Type", "application/json; charset=utf-8")
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            anyhow::bail!("Slack API returned HTTP {}", status);
        }

        let result: serde_json::Value = resp.json().await?;
        if result.get("ok").and_then(|v| v.as_bool()) != Some(true) {
            let error = result
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            anyhow::bail!("Slack API error: {}", error);
        }

        Ok(())
    }

    fn surface_type(&self) -> SurfaceType {
        SurfaceType::Slack
    }
}
