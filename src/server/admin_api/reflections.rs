use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use std::sync::Arc;

use crate::store::models::{Agent, ReflectionReport};

/// POST /api/tenants/{tid}/agents/{aid}/reflect — trigger a reflection
pub async fn trigger_reflection(
    State(state): State<Arc<crate::server::AppState>>,
    Path((tid_str, aid_str)): Path<(String, String)>,
) -> impl IntoResponse {
    let tenant_id = match super::resolve_tenant_id(&state.platform_store, &tid_str) {
        Ok(id) => id,
        Err(e) => return e,
    };

    let tenant_store = match state.open_tenant_store(&tenant_id) {
        Ok(s) => s,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
        }
    };

    let agent_id = match super::resolve_agent_id(&tenant_store, tenant_id, &aid_str) {
        Ok(id) => id,
        Err(e) => return e,
    };

    let conn = tenant_store.conn();

    let agent = match Agent::get_current(conn, tenant_id, agent_id) {
        Ok(Some(a)) => a,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "agent not found" })),
            )
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
        }
    };

    // Use the last 24 hours as the reflection window
    let now = chrono::Utc::now();
    let window_start = (now - chrono::Duration::hours(24)).to_rfc3339();
    let window_end = now.to_rfc3339();

    match crate::engine::reflection::run_reflection(
        conn,
        tenant_id,
        &agent,
        "manual",
        &window_start,
        &window_end,
    ) {
        Ok(report) => (
            StatusCode::CREATED,
            Json(serde_json::to_value(&report).unwrap()),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ),
    }
}

/// GET /api/tenants/{tid}/agents/{aid}/reflections — list reflection reports
pub async fn list_reflections(
    State(state): State<Arc<crate::server::AppState>>,
    Path((tid_str, aid_str)): Path<(String, String)>,
) -> impl IntoResponse {
    let tenant_id: uuid::Uuid = match super::resolve_tenant_id(&state.platform_store, &tid_str) {
        Ok(id) => id,
        Err(_) => return Json(serde_json::json!([])),
    };

    let tenant_store = match state.open_tenant_store(&tenant_id) {
        Ok(s) => s,
        Err(_) => return Json(serde_json::json!([])),
    };

    let agent_id: uuid::Uuid = match super::resolve_agent_id(&tenant_store, tenant_id, &aid_str) {
        Ok(id) => id,
        Err(_) => return Json(serde_json::json!([])),
    };

    match ReflectionReport::list_by_agent(tenant_store.conn(), tenant_id, agent_id) {
        Ok(reports) => Json(serde_json::to_value(&reports).unwrap()),
        Err(_) => Json(serde_json::json!([])),
    }
}

/// GET /api/tenants/{tid}/agents/{aid}/reflections/{rid} — get a single report
pub async fn get_reflection(
    State(state): State<Arc<crate::server::AppState>>,
    Path((tid_str, _aid_str, rid)): Path<(String, String, String)>,
) -> impl IntoResponse {
    let tenant_id = match super::resolve_tenant_id(&state.platform_store, &tid_str) {
        Ok(id) => id,
        Err(e) => return e,
    };

    let report_id: uuid::Uuid = match rid.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "invalid report id" })),
            )
        }
    };

    let tenant_store = match state.open_tenant_store(&tenant_id) {
        Ok(s) => s,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
        }
    };

    match ReflectionReport::get(tenant_store.conn(), report_id) {
        Ok(Some(report)) => (StatusCode::OK, Json(serde_json::to_value(&report).unwrap())),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "report not found" })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ),
    }
}

/// DELETE /api/tenants/{tid}/agents/{aid}/reflections/{rid} — delete a report
pub async fn delete_reflection(
    State(state): State<Arc<crate::server::AppState>>,
    Path((tid_str, _aid_str, rid)): Path<(String, String, String)>,
) -> impl IntoResponse {
    let tenant_id = match super::resolve_tenant_id(&state.platform_store, &tid_str) {
        Ok(id) => id,
        Err(_) => return StatusCode::NOT_FOUND,
    };

    let report_id: uuid::Uuid = match rid.parse() {
        Ok(id) => id,
        Err(_) => return StatusCode::BAD_REQUEST,
    };

    let tenant_store = match state.open_tenant_store(&tenant_id) {
        Ok(s) => s,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR,
    };

    match ReflectionReport::delete(tenant_store.conn(), report_id) {
        Ok(_) => StatusCode::NO_CONTENT,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

/// POST /api/tenants/{tid}/agents/{aid}/memory/promote — promote memory entry to tenant scope
pub async fn promote_memory(
    State(state): State<Arc<crate::server::AppState>>,
    Path((tid_str, aid_str)): Path<(String, String)>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let tenant_id = match super::resolve_tenant_id(&state.platform_store, &tid_str) {
        Ok(id) => id,
        Err(e) => return e,
    };

    let tenant_store = match state.open_tenant_store(&tenant_id) {
        Ok(s) => s,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
        }
    };

    let agent_id = match super::resolve_agent_id(&tenant_store, tenant_id, &aid_str) {
        Ok(id) => id,
        Err(e) => return e,
    };

    let entry_id_str = match body.get("entry_id").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "entry_id required" })),
            )
        }
    };

    let entry_id: uuid::Uuid = match entry_id_str.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "invalid entry_id" })),
            )
        }
    };

    match crate::engine::memory::promote_to_tenant(
        tenant_store.conn(),
        &entry_id,
        &tenant_id,
        &agent_id,
    ) {
        Ok(_) => (
            StatusCode::OK,
            Json(serde_json::json!({ "status": "promoted" })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ),
    }
}

/// POST /api/tenants/{tid}/agents/{aid}/users/{uid}/offboard — offboard a user
pub async fn offboard_user(
    State(state): State<Arc<crate::server::AppState>>,
    Path((tid_str, _aid_str, uid)): Path<(String, String, String)>,
    Json(body): Json<serde_json::Value>,
) -> impl IntoResponse {
    let tenant_id = match super::resolve_tenant_id(&state.platform_store, &tid_str) {
        Ok(id) => id,
        Err(e) => return e,
    };

    let memory_action = body
        .get("memory")
        .and_then(|v| v.as_str())
        .unwrap_or("keep");

    let tenant_store = match state.open_tenant_store(&tenant_id) {
        Ok(s) => s,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": e.to_string() })),
            )
        }
    };

    let conn = tenant_store.conn();
    let user_scope = format!("user:{}", uid);

    match memory_action {
        "delete-now" => {
            // Delete all user-scoped memory entries for this user
            let result = conn.execute(
                "DELETE FROM memory_entries WHERE tenant_id = ?1 AND scope = ?2",
                rusqlite::params![tenant_id.to_string(), user_scope],
            );
            match result {
                Ok(count) => (
                    StatusCode::OK,
                    Json(serde_json::json!({
                        "status": "offboarded",
                        "user": uid,
                        "memory_action": "delete-now",
                        "entries_deleted": count,
                    })),
                ),
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "error": e.to_string() })),
                ),
            }
        }
        _ => {
            // "keep" — just mark the user as offboarded without deleting memories
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "status": "offboarded",
                    "user": uid,
                    "memory_action": "keep",
                })),
            )
        }
    }
}
