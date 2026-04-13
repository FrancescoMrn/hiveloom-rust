//! Admin API endpoints for compaction (T022, T023).
//!
//! - GET  /api/tenants/:tid/compaction-events — list compaction events
//! - GET  /api/tenants/:tid/agents/:aid/compaction-config — get resolved config
//! - PATCH /api/tenants/:tid/agents/:aid/compaction-config — update config

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::compaction::config::{self, CompactionConfig, ResolvedCompactionConfig};
use crate::compaction::event::CompactionEvent;
use crate::server::AppState;

// ── GET /api/tenants/:tid/compaction-events (T022) ─────────────────────

#[derive(Debug, Deserialize)]
pub struct CompactionEventQuery {
    pub agent_id: Option<String>,
    pub since: Option<String>,
    pub limit: Option<i64>,
}

pub async fn list_compaction_events(
    State(state): State<Arc<AppState>>,
    Path(tid_str): Path<String>,
    Query(query): Query<CompactionEventQuery>,
) -> impl IntoResponse {
    let tenant_id = match super::resolve_tenant_id(&state.platform_store, &tid_str) {
        Ok(id) => id,
        Err(e) => return e.into_response(),
    };

    let store = match state.open_tenant_store(&tenant_id) {
        Ok(s) => s,
        Err(_) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "tenant not found"})),
            )
                .into_response()
        }
    };

    let agent_id = query.agent_id.and_then(|s| Uuid::parse_str(&s).ok());
    let limit = query.limit.unwrap_or(50);

    match CompactionEvent::list(
        store.conn(),
        tenant_id,
        agent_id,
        query.since.as_deref(),
        limit,
    ) {
        Ok(events) => Json(serde_json::to_value(&events).unwrap()).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

// ── GET /api/tenants/:tid/agents/:aid/compaction-config (T023) ─────────

#[derive(Debug, Serialize)]
struct CompactionConfigResponse {
    #[serde(flatten)]
    config: ResolvedCompactionConfig,
}

pub async fn get_compaction_config(
    State(state): State<Arc<AppState>>,
    Path((tid_str, aid_str)): Path<(String, String)>,
) -> impl IntoResponse {
    let tenant_id = match super::resolve_tenant_id(&state.platform_store, &tid_str) {
        Ok(id) => id,
        Err(e) => return e.into_response(),
    };

    let store = match state.open_tenant_store(&tenant_id) {
        Ok(s) => s,
        Err(_) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "tenant not found"})),
            )
                .into_response()
        }
    };

    let agent_id = match super::resolve_agent_id(&store, tenant_id, &aid_str) {
        Ok(id) => id,
        Err(e) => return e.into_response(),
    };

    match config::resolve_config(store.conn(), tenant_id, agent_id) {
        Ok(resolved) => {
            Json(serde_json::to_value(&CompactionConfigResponse { config: resolved }).unwrap())
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

// ── PATCH /api/tenants/:tid/agents/:aid/compaction-config (T023) ───────

#[derive(Debug, Deserialize)]
pub struct PatchCompactionConfig {
    pub threshold_pct: Option<i64>,
    pub max_summary_fraction_pct: Option<i64>,
    pub protected_turn_count: Option<i64>,
    pub show_indicator: Option<bool>,
    #[serde(default)]
    pub reset: bool,
}

pub async fn patch_compaction_config(
    State(state): State<Arc<AppState>>,
    Path((tid_str, aid_str)): Path<(String, String)>,
    Json(body): Json<PatchCompactionConfig>,
) -> impl IntoResponse {
    let tenant_id = match super::resolve_tenant_id(&state.platform_store, &tid_str) {
        Ok(id) => id,
        Err(e) => return e.into_response(),
    };

    let store = match state.open_tenant_store(&tenant_id) {
        Ok(s) => s,
        Err(_) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "tenant not found"})),
            )
                .into_response()
        }
    };

    let agent_id = match super::resolve_agent_id(&store, tenant_id, &aid_str) {
        Ok(id) => id,
        Err(e) => return e.into_response(),
    };

    let conn = store.conn();

    // Handle --reset: delete agent override
    if body.reset {
        if let Err(e) = CompactionConfig::delete_for_agent(conn, tenant_id, agent_id) {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e.to_string()})),
            )
                .into_response();
        }
    } else {
        // Check if an agent-level config exists
        match CompactionConfig::get_for_agent(conn, tenant_id, agent_id) {
            Ok(Some(existing)) => {
                // Update existing
                if let Err(e) = CompactionConfig::update(
                    conn,
                    existing.id,
                    body.threshold_pct,
                    body.max_summary_fraction_pct,
                    body.protected_turn_count,
                    body.show_indicator,
                ) {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(serde_json::json!({"error": e.to_string()})),
                    )
                        .into_response();
                }
            }
            Ok(None) => {
                // Create new agent-level override
                let threshold = body.threshold_pct.unwrap_or(config::DEFAULT_THRESHOLD_PCT);
                let max_summary = body
                    .max_summary_fraction_pct
                    .unwrap_or(config::DEFAULT_MAX_SUMMARY_FRACTION_PCT);
                let protected = body
                    .protected_turn_count
                    .unwrap_or(config::DEFAULT_PROTECTED_TURN_COUNT);
                let indicator = body
                    .show_indicator
                    .unwrap_or(config::DEFAULT_SHOW_INDICATOR);

                if let Err(e) = CompactionConfig::create(
                    conn,
                    tenant_id,
                    Some(agent_id),
                    threshold,
                    max_summary,
                    protected,
                    indicator,
                ) {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(serde_json::json!({"error": e.to_string()})),
                    )
                        .into_response();
                }
            }
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": e.to_string()})),
                )
                    .into_response();
            }
        }
    }

    // Return resolved config after update
    match config::resolve_config(conn, tenant_id, agent_id) {
        Ok(resolved) => {
            Json(serde_json::to_value(&CompactionConfigResponse { config: resolved }).unwrap())
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}
