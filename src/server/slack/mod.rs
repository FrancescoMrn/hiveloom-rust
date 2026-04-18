pub mod auth;
pub mod events;

use axum::{
    body::Bytes,
    extract::{Query, State},
    http::HeaderMap,
    Json,
};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::sync::Arc;

use crate::engine::chat_surface::{ChatSurface, SurfaceType};
use crate::store::models::{CredentialVaultEntry, Tenant};

type HmacSha256 = Hmac<Sha256>;
pub const ACCESS_TOKEN_CREDENTIAL_NAME: &str = "slack_access_token";
pub const DEFAULT_INSTALL_SCOPES: &str =
    "app_mentions:read,channels:history,chat:write,groups:history,im:history,mpim:history";

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

            // Spawn dispatch in the background so we respond 200 quickly
            tokio::spawn(async move {
                if let Err(e) = events::dispatch_event(&state, &slack_event).await {
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

#[derive(Serialize)]
pub struct SlackSetupStatus {
    pub configured: bool,
    pub tenant: Option<String>,
    pub signing_secret_present: bool,
    pub client_id_present: bool,
    pub client_secret_present: bool,
    pub app_id_present: bool,
    pub access_token_present: bool,
    pub access_token_source: Option<String>,
    pub access_token_kind: Option<String>,
    pub install_ready: bool,
    pub install_url: Option<String>,
    pub oauth_redirect_url: Option<String>,
    pub events_url: String,
    pub observed_base_url: String,
    pub observed_https: bool,
    pub real_delivery_ready: bool,
    pub channel_invite_ready: bool,
    pub notes: Vec<String>,
}

fn token_kind_label(kind: &crate::server::SlackTokenKind) -> &'static str {
    match kind {
        crate::server::SlackTokenKind::Bot => "bot",
        crate::server::SlackTokenKind::User => "user",
        crate::server::SlackTokenKind::App => "app",
        crate::server::SlackTokenKind::Unknown => "unknown",
    }
}

pub struct ResolvedSlackAccessToken {
    pub value: String,
    pub source: String,
    pub kind: crate::server::SlackTokenKind,
}

pub(crate) fn resolve_access_token(
    state: &crate::server::AppState,
    conn: &rusqlite::Connection,
    tenant_id: uuid::Uuid,
) -> anyhow::Result<Option<ResolvedSlackAccessToken>> {
    if let Some(entry) =
        CredentialVaultEntry::get_by_name(conn, tenant_id, ACCESS_TOKEN_CREDENTIAL_NAME, None)?
    {
        let decrypted = state.vault.decrypt(&entry.encrypted_value)?;
        let value = String::from_utf8(decrypted)?;
        return Ok(Some(ResolvedSlackAccessToken {
            kind: crate::server::slack_token_kind(&value),
            source: format!("credential:{}", ACCESS_TOKEN_CREDENTIAL_NAME),
            value,
        }));
    }

    if let Some(config) = state.slack_config.as_ref() {
        if let Some(value) = config.access_token.as_ref() {
            return Ok(Some(ResolvedSlackAccessToken {
                kind: config
                    .access_token_kind
                    .clone()
                    .unwrap_or_else(|| crate::server::slack_token_kind(value)),
                source: config
                    .access_token_source
                    .unwrap_or("SLACK_ACCESS_TOKEN")
                    .to_string(),
                value: value.clone(),
            }));
        }
    }

    Ok(None)
}

#[derive(Default, Deserialize)]
pub struct SlackSetupQuery {
    pub tenant: Option<String>,
}

fn build_setup_status(
    observed_base_url: String,
    tenant_slug: Option<String>,
    signing_secret_present: bool,
    client_id_present: bool,
    client_secret_present: bool,
    app_id_present: bool,
    access_token: Option<&ResolvedSlackAccessToken>,
) -> SlackSetupStatus {
    let observed_https = observed_base_url.starts_with("https://");
    let mut notes = Vec::new();
    let install_ready = signing_secret_present && client_id_present && client_secret_present;
    let access_token_present = access_token.is_some();

    if !observed_https {
        notes.push(
            "Slack Event Subscriptions require a public HTTPS URL. Put Hiveloom behind Caddy, \
             Nginx, or another TLS terminator before configuring Slack."
                .to_string(),
        );
    }

    if !signing_secret_present {
        notes.push(
            "Set SLACK_SIGNING_SECRET to verify incoming Slack event signatures.".to_string(),
        );
    }
    if !client_id_present || !client_secret_present {
        notes.push(
            "Set both SLACK_CLIENT_ID and SLACK_CLIENT_SECRET to enable the Slack app install flow."
                .to_string(),
        );
    }
    if !access_token_present {
        notes.push(
            "Install the Slack app through /slack/install or provide SLACK_ACCESS_TOKEN as a fallback token."
                .to_string(),
        );
    }
    if let Some(token) = access_token {
        if token.kind == crate::server::SlackTokenKind::User {
            notes.push(
                "A user token can post replies, but a bot token is the recommended choice for workspace installs where the app is invited into support channels."
                    .to_string(),
            );
        }
    }
    if !app_id_present {
        notes.push("SLACK_APP_ID is optional but useful for operator visibility.".to_string());
    }

    SlackSetupStatus {
        configured: signing_secret_present && access_token_present,
        tenant: tenant_slug.clone(),
        signing_secret_present,
        client_id_present,
        client_secret_present,
        app_id_present,
        access_token_present,
        access_token_source: access_token.map(|token| token.source.clone()),
        access_token_kind: access_token.map(|token| token_kind_label(&token.kind).to_string()),
        install_ready,
        install_url: if install_ready {
            tenant_slug
                .as_ref()
                .map(|tenant| format!("{}/slack/install?tenant={}", observed_base_url, tenant))
        } else {
            None
        },
        oauth_redirect_url: if install_ready {
            Some(format!("{}/slack/oauth/callback", observed_base_url))
        } else {
            None
        },
        events_url: format!("{}/slack/events", observed_base_url),
        observed_base_url: observed_base_url.clone(),
        observed_https,
        real_delivery_ready: signing_secret_present && access_token_present && observed_https,
        channel_invite_ready: access_token_present,
        notes,
    }
}

/// GET /slack/setup — operator-facing setup status for real Slack delivery.
pub async fn setup_status(
    State(state): State<Arc<super::AppState>>,
    Query(query): Query<SlackSetupQuery>,
    headers: HeaderMap,
) -> Json<SlackSetupStatus> {
    let observed_base_url = crate::server::external_base_url(&headers);
    let tenant_slug = query.tenant.unwrap_or_else(|| "default".to_string());
    let mut notes = Vec::new();

    let tenant_id = {
        let conn = state.platform_store.conn();
        match Tenant::get_by_slug(&conn, &tenant_slug) {
            Ok(Some(tenant)) => Some(tenant.id),
            Ok(None) => {
                notes.push(format!("Tenant '{}' was not found.", tenant_slug));
                None
            }
            Err(e) => {
                notes.push(format!("Could not inspect tenant '{}': {}", tenant_slug, e));
                None
            }
        }
    };

    let access_token = if let Some(tid) = tenant_id {
        match state.open_tenant_store(&tid) {
            Ok(tenant_store) => match resolve_access_token(&state, tenant_store.conn(), tid) {
                Ok(token) => token,
                Err(e) => {
                    notes.push(format!("Could not load Slack access token: {}", e));
                    None
                }
            },
            Err(e) => {
                notes.push(format!("Could not open tenant store: {}", e));
                None
            }
        }
    } else {
        None
    };

    let signing_secret_present = state.slack_config.is_some();
    let client_id_present = state
        .slack_config
        .as_ref()
        .and_then(|config| config.client_id.as_ref())
        .is_some();
    let client_secret_present = state
        .slack_config
        .as_ref()
        .and_then(|config| config.client_secret.as_ref())
        .is_some();
    let app_id_present = state
        .slack_config
        .as_ref()
        .and_then(|config| config.app_id.as_ref())
        .is_some();

    let mut status = build_setup_status(
        observed_base_url,
        Some(tenant_slug),
        signing_secret_present,
        client_id_present,
        client_secret_present,
        app_id_present,
        access_token.as_ref(),
    );
    status.notes.extend(notes);
    Json(status)
}

// ── T040: SlackSurface ──────────────────────────────────────────────────

pub struct SlackSurface {
    access_token: String,
    client: reqwest::Client,
}

impl SlackSurface {
    pub fn new(access_token: &str) -> Self {
        Self {
            access_token: access_token.to_string(),
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
            .header("Authorization", format!("Bearer {}", self.access_token))
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

#[cfg(test)]
mod tests {
    use super::{build_setup_status, ResolvedSlackAccessToken};

    #[test]
    fn setup_status_reports_https_readiness_from_forwarded_headers() {
        let token = ResolvedSlackAccessToken {
            value: "xoxb-test".to_string(),
            source: "credential:slack_access_token".to_string(),
            kind: crate::server::SlackTokenKind::Bot,
        };
        let status = build_setup_status(
            "https://support.example.com".to_string(),
            Some("default".to_string()),
            true,
            true,
            true,
            true,
            Some(&token),
        );
        assert_eq!(
            status.events_url,
            "https://support.example.com/slack/events"
        );
        assert!(status.observed_https);
        assert!(status.real_delivery_ready);
        assert_eq!(
            status.install_url.as_deref(),
            Some("https://support.example.com/slack/install?tenant=default")
        );
    }
}
