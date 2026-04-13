use axum::Router;
use std::sync::Arc;

pub mod admin_api;
pub mod healthz;
pub mod mcp;
pub mod oauth;
pub mod slack;

pub struct AppState {
    pub data_dir: String,
    pub platform_store: crate::store::PlatformStore,
}

impl AppState {
    pub async fn new(data_dir: &str) -> anyhow::Result<Self> {
        let platform_store = crate::store::PlatformStore::open(std::path::Path::new(data_dir))?;
        Ok(Self {
            data_dir: data_dir.to_string(),
            platform_store,
        })
    }
}

pub fn create_router(state: AppState) -> Router {
    let shared_state = Arc::new(state);
    Router::new()
        .nest("/api", admin_api::router(shared_state.clone()))
        .route("/healthz", axum::routing::get(healthz::handler))
        // Slack and MCP routes will be added in later phases
        .with_state(shared_state)
}
