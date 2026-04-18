use axum::{http::HeaderMap, Router};
use std::sync::Arc;

pub mod admin_api;
pub mod events;
pub mod healthz;
pub mod mcp;
pub mod oauth;
pub mod slack;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SlackTokenKind {
    Bot,
    User,
    App,
    Unknown,
}

pub struct SlackConfig {
    pub signing_secret: String,
    pub access_token: Option<String>,
    pub access_token_source: Option<&'static str>,
    pub access_token_kind: Option<SlackTokenKind>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub app_id: Option<String>,
}

pub struct AppState {
    pub data_dir: String,
    pub platform_store: crate::store::PlatformStore,
    pub vault: crate::store::Vault,
    pub dedup: crate::engine::DedupTable,
    pub slack_config: Option<SlackConfig>,
}

fn env_var_if_set(name: &'static str) -> Option<(&'static str, String)> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(|value| (name, value))
}

pub fn slack_token_kind(token: &str) -> SlackTokenKind {
    if token.starts_with("xoxb-") {
        SlackTokenKind::Bot
    } else if token.starts_with("xoxp-") {
        SlackTokenKind::User
    } else if token.starts_with("xapp-") {
        SlackTokenKind::App
    } else {
        SlackTokenKind::Unknown
    }
}

pub fn header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_string())
}

pub fn external_base_url(headers: &HeaderMap) -> String {
    let proto = header_value(headers, "x-forwarded-proto")
        .or_else(|| header_value(headers, "x-forwarded-protocol"))
        .unwrap_or_else(|| "http".to_string());
    let host = header_value(headers, "x-forwarded-host")
        .or_else(|| header_value(headers, "host"))
        .unwrap_or_else(|| "127.0.0.1:3000".to_string());
    format!("{}://{}", proto, host)
}

impl AppState {
    pub async fn new(data_dir: &str) -> anyhow::Result<Self> {
        let platform_store = crate::store::PlatformStore::open(std::path::Path::new(data_dir))?;
        let vault = crate::store::Vault::open(std::path::Path::new(data_dir))?;
        let dedup = crate::engine::DedupTable::new();

        // Load Slack config from environment if available.
        // `SLACK_ACCESS_TOKEN` is the preferred name because either a bot token
        // (`xoxb-...`) or a user token (`xoxp-...`) can work for outbound posting.
        let slack_signing_secret = env_var_if_set("SLACK_SIGNING_SECRET");
        let slack_access_token = env_var_if_set("SLACK_ACCESS_TOKEN")
            .or_else(|| env_var_if_set("SLACK_BOT_TOKEN"))
            .or_else(|| env_var_if_set("SLACK_USER_TOKEN"));
        let slack_client_id = env_var_if_set("SLACK_CLIENT_ID").map(|(_, value)| value);
        let slack_client_secret = env_var_if_set("SLACK_CLIENT_SECRET").map(|(_, value)| value);
        let slack_app_id = env_var_if_set("SLACK_APP_ID").map(|(_, value)| value);

        let slack_config = match slack_signing_secret {
            Some((_, secret)) => {
                let (access_token, access_token_source, access_token_kind) =
                    match slack_access_token {
                        Some((token_source, access_token)) => {
                            let token_kind = slack_token_kind(&access_token);
                            if token_source != "SLACK_ACCESS_TOKEN" {
                                tracing::info!(
                                    token_source,
                                    "Loaded Slack access token from legacy environment variable"
                                );
                            }
                            if token_kind == SlackTokenKind::User {
                                tracing::warn!(
                                    "Slack is configured with a user token; this can work for channel support, \
                                     but a bot token is recommended for workspace installs"
                                );
                            }
                            (Some(access_token), Some(token_source), Some(token_kind))
                        }
                        None => (None, None, None),
                    };

                if slack_client_id.is_some() ^ slack_client_secret.is_some() {
                    tracing::warn!(
                        "Slack install configuration is incomplete; set both SLACK_CLIENT_ID and \
                         SLACK_CLIENT_SECRET to enable workspace app installation"
                    );
                }

                Some(SlackConfig {
                    signing_secret: secret,
                    access_token,
                    access_token_source,
                    access_token_kind,
                    client_id: slack_client_id,
                    client_secret: slack_client_secret,
                    app_id: slack_app_id,
                })
            }
            None => {
                if slack_access_token.is_some()
                    || slack_client_id.is_some()
                    || slack_client_secret.is_some()
                    || slack_app_id.is_some()
                {
                    tracing::warn!(
                        "Slack configuration is incomplete; set SLACK_SIGNING_SECRET in addition to \
                         SLACK_ACCESS_TOKEN and/or Slack app install credentials"
                    );
                }
                None
            }
        };

        Ok(Self {
            data_dir: data_dir.to_string(),
            platform_store,
            vault,
            dedup,
            slack_config,
        })
    }

    /// Open a TenantStore for the given tenant id.
    pub fn open_tenant_store(
        &self,
        tenant_id: &uuid::Uuid,
    ) -> anyhow::Result<crate::store::TenantStore> {
        crate::store::TenantStore::open(std::path::Path::new(&self.data_dir), tenant_id)
    }
}

pub fn create_router(state: AppState) -> Router {
    let shared_state = Arc::new(state);
    Router::new()
        .nest("/api", admin_api::router(shared_state.clone()))
        .route("/healthz", axum::routing::get(healthz::handler))
        .route("/slack/install", axum::routing::get(slack::auth::start_install))
        .route(
            "/slack/oauth/callback",
            axum::routing::get(slack::auth::handle_install_callback),
        )
        .route("/slack/setup", axum::routing::get(slack::setup_status))
        .route("/slack/events", axum::routing::post(slack::handle_event))
        .route(
            "/events/:tid/inbound",
            axum::routing::post(events::handle_inbound_event),
        )
        // ── OAuth callback (T075) ──────────────────────────────────────
        .route(
            "/oauth/callback",
            axum::routing::get(oauth::handle_callback),
        )
        // ── MCP Streamable HTTP transport (T082) ───────────────────────
        .route(
            "/mcp/:tenant_slug/:agent_slug",
            axum::routing::post(mcp::transport::handle_mcp_request),
        )
        // ── OAuth Discovery (T085) ────────────────────────────────────
        .route(
            "/.well-known/oauth-authorization-server",
            axum::routing::get(mcp::auth::oauth_metadata),
        )
        .route(
            "/mcp/:tenant_slug/:agent_slug/.well-known/oauth-protected-resource",
            axum::routing::get(mcp::auth::protected_resource_metadata),
        )
        // ── OAuth Endpoints (spec-compliant) ──────────────────────────
        .route(
            "/oauth/register",
            axum::routing::post(mcp::auth::register_client),
        )
        .route(
            "/oauth/authorize",
            axum::routing::get(mcp::auth::authorize)
                .post(mcp::auth::authorize_submit),
        )
        .route(
            "/oauth/token",
            axum::routing::post(mcp::auth::token),
        )
        .with_state(shared_state)
}

#[cfg(test)]
mod tests {
    use super::{slack_token_kind, SlackTokenKind};

    #[test]
    fn classifies_known_slack_token_prefixes() {
        assert_eq!(slack_token_kind("xoxb-test"), SlackTokenKind::Bot);
        assert_eq!(slack_token_kind("xoxp-test"), SlackTokenKind::User);
        assert_eq!(slack_token_kind("xapp-test"), SlackTokenKind::App);
        assert_eq!(slack_token_kind("something-else"), SlackTokenKind::Unknown);
    }
}
