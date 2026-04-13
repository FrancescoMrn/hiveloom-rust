use axum::Router;
use std::sync::Arc;

pub mod admin_api;
pub mod healthz;
pub mod mcp;
pub mod oauth;
pub mod slack;

pub struct SlackConfig {
    pub signing_secret: String,
    pub bot_token: String,
}

pub struct AppState {
    pub data_dir: String,
    pub platform_store: crate::store::PlatformStore,
    pub vault: crate::store::Vault,
    pub dedup: crate::engine::DedupTable,
    pub slack_config: Option<SlackConfig>,
}

impl AppState {
    pub async fn new(data_dir: &str) -> anyhow::Result<Self> {
        let platform_store = crate::store::PlatformStore::open(std::path::Path::new(data_dir))?;
        let vault = crate::store::Vault::open(std::path::Path::new(data_dir))?;
        let dedup = crate::engine::DedupTable::new();

        // Load Slack config from environment if available
        let slack_config = match (
            std::env::var("SLACK_SIGNING_SECRET"),
            std::env::var("SLACK_BOT_TOKEN"),
        ) {
            (Ok(secret), Ok(token)) => Some(SlackConfig {
                signing_secret: secret,
                bot_token: token,
            }),
            _ => None,
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
        .route("/slack/events", axum::routing::post(slack::handle_event))
        .with_state(shared_state)
}
