use axum::{
    extract::{Json, Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::Deserialize;
use std::sync::Arc;

use crate::store::models::{Agent, ChatSurfaceBinding};

fn err_json(status: StatusCode, msg: &str) -> (StatusCode, Json<serde_json::Value>) {
    (status, Json(serde_json::json!({ "error": msg })))
}

// ── Agent CRUD ──────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateAgentRequest {
    pub name: String,
    #[serde(default)]
    pub system_prompt: String,
    pub model_id: String,
    #[serde(default = "default_scope_mode")]
    pub scope_mode: String,
    #[serde(default = "default_scope_policy")]
    pub default_scope_policy: String,
    #[serde(default = "default_coerce_policy")]
    pub scope_coerce_policy: String,
    #[serde(default)]
    pub reflection_enabled: bool,
    pub reflection_cron: Option<String>,
}

fn default_scope_mode() -> String {
    "dual".to_string()
}
fn default_scope_policy() -> String {
    "tenant".to_string()
}
fn default_coerce_policy() -> String {
    "coerce".to_string()
}

#[derive(Deserialize)]
pub struct UpdateAgentRequest {
    pub name: String,
    pub system_prompt: String,
    pub model_id: String,
    #[serde(default = "default_scope_mode")]
    pub scope_mode: String,
    #[serde(default = "default_scope_policy")]
    pub default_scope_policy: String,
    #[serde(default = "default_coerce_policy")]
    pub scope_coerce_policy: String,
    #[serde(default)]
    pub reflection_enabled: bool,
    pub reflection_cron: Option<String>,
}

#[derive(Deserialize)]
pub struct RollbackRequest {
    pub version: i64,
}

pub async fn create_agent(
    State(state): State<Arc<super::super::AppState>>,
    Path(tid_str): Path<String>,
    Json(body): Json<CreateAgentRequest>,
) -> impl IntoResponse {
    let tid = match super::resolve_tenant_id(&state.platform_store, &tid_str) {
        Ok(id) => id,
        Err(e) => return e,
    };
    let tenant_store = match state.open_tenant_store(&tid) {
        Ok(s) => s,
        Err(e) => return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    let conn = tenant_store.conn();
    match Agent::create(
        conn,
        tid,
        &body.name,
        &body.system_prompt,
        &body.model_id,
        &body.scope_mode,
        &body.default_scope_policy,
        &body.scope_coerce_policy,
        body.reflection_enabled,
        body.reflection_cron.as_deref(),
    ) {
        Ok(agent) => (
            StatusCode::CREATED,
            Json(serde_json::to_value(agent).unwrap()),
        ),
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

pub async fn list_agents(
    State(state): State<Arc<super::super::AppState>>,
    Path(tid_str): Path<String>,
) -> impl IntoResponse {
    let tid = match super::resolve_tenant_id(&state.platform_store, &tid_str) {
        Ok(id) => id,
        Err(e) => return e,
    };
    let tenant_store = match state.open_tenant_store(&tid) {
        Ok(s) => s,
        Err(e) => return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    let conn = tenant_store.conn();
    match Agent::list_current(conn, tid) {
        Ok(agents) => (StatusCode::OK, Json(serde_json::to_value(agents).unwrap())),
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

pub async fn get_agent(
    State(state): State<Arc<super::super::AppState>>,
    Path((tid_str, aid_str)): Path<(String, String)>,
) -> impl IntoResponse {
    let tid = match super::resolve_tenant_id(&state.platform_store, &tid_str) {
        Ok(id) => id,
        Err(e) => return e,
    };
    let tenant_store = match state.open_tenant_store(&tid) {
        Ok(s) => s,
        Err(e) => return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    let aid = match super::resolve_agent_id(&tenant_store, tid, &aid_str) {
        Ok(id) => id,
        Err(e) => return e,
    };
    let conn = tenant_store.conn();
    match Agent::get_current(conn, tid, aid) {
        Ok(Some(agent)) => (StatusCode::OK, Json(serde_json::to_value(agent).unwrap())),
        Ok(None) => err_json(StatusCode::NOT_FOUND, "Agent not found"),
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

pub async fn update_agent(
    State(state): State<Arc<super::super::AppState>>,
    Path((tid_str, aid_str)): Path<(String, String)>,
    Json(body): Json<UpdateAgentRequest>,
) -> impl IntoResponse {
    let tid = match super::resolve_tenant_id(&state.platform_store, &tid_str) {
        Ok(id) => id,
        Err(e) => return e,
    };
    let tenant_store = match state.open_tenant_store(&tid) {
        Ok(s) => s,
        Err(e) => return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    let aid = match super::resolve_agent_id(&tenant_store, tid, &aid_str) {
        Ok(id) => id,
        Err(e) => return e,
    };
    let conn = tenant_store.conn();

    // Load current version
    let current = match Agent::get_current(conn, tid, aid) {
        Ok(Some(a)) => a,
        Ok(None) => return err_json(StatusCode::NOT_FOUND, "Agent not found"),
        Err(e) => return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };

    // Build updated agent struct and create new version
    let mut updated = current;
    updated.name = body.name;
    updated.system_prompt = body.system_prompt;
    updated.model_id = body.model_id;
    updated.scope_mode = body.scope_mode;
    updated.default_scope_policy = body.default_scope_policy;
    updated.scope_coerce_policy = body.scope_coerce_policy;
    updated.reflection_enabled = body.reflection_enabled;
    updated.reflection_cron = body.reflection_cron;

    match Agent::update(conn, &updated) {
        Ok(new_ver) => (StatusCode::OK, Json(serde_json::to_value(new_ver).unwrap())),
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

pub async fn delete_agent(
    State(state): State<Arc<super::super::AppState>>,
    Path((tid_str, aid_str)): Path<(String, String)>,
) -> impl IntoResponse {
    let tid = match super::resolve_tenant_id(&state.platform_store, &tid_str) {
        Ok(id) => id,
        Err(e) => return e,
    };
    let tenant_store = match state.open_tenant_store(&tid) {
        Ok(s) => s,
        Err(e) => return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    let aid = match super::resolve_agent_id(&tenant_store, tid, &aid_str) {
        Ok(id) => id,
        Err(e) => return e,
    };
    let conn = tenant_store.conn();
    match Agent::delete(conn, aid) {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({ "deleted": true }))),
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

pub async fn list_versions(
    State(state): State<Arc<super::super::AppState>>,
    Path((tid_str, aid_str)): Path<(String, String)>,
) -> impl IntoResponse {
    let tid = match super::resolve_tenant_id(&state.platform_store, &tid_str) {
        Ok(id) => id,
        Err(e) => return e,
    };
    let tenant_store = match state.open_tenant_store(&tid) {
        Ok(s) => s,
        Err(e) => return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    let aid = match super::resolve_agent_id(&tenant_store, tid, &aid_str) {
        Ok(id) => id,
        Err(e) => return e,
    };
    let conn = tenant_store.conn();
    match Agent::list_versions(conn, aid) {
        Ok(versions) => (
            StatusCode::OK,
            Json(serde_json::to_value(versions).unwrap()),
        ),
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

pub async fn rollback_agent(
    State(state): State<Arc<super::super::AppState>>,
    Path((tid_str, aid_str)): Path<(String, String)>,
    Json(body): Json<RollbackRequest>,
) -> impl IntoResponse {
    let tid = match super::resolve_tenant_id(&state.platform_store, &tid_str) {
        Ok(id) => id,
        Err(e) => return e,
    };
    let tenant_store = match state.open_tenant_store(&tid) {
        Ok(s) => s,
        Err(e) => return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    let aid = match super::resolve_agent_id(&tenant_store, tid, &aid_str) {
        Ok(id) => id,
        Err(e) => return e,
    };
    let conn = tenant_store.conn();
    match Agent::rollback(conn, aid, body.version) {
        Ok(agent) => (StatusCode::OK, Json(serde_json::to_value(agent).unwrap())),
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

// ── T045: ChatSurfaceBinding API ────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateBindingRequest {
    pub surface_type: String,
    pub surface_ref: String,
}

pub async fn create_binding(
    State(state): State<Arc<super::super::AppState>>,
    Path((tid_str, aid_str)): Path<(String, String)>,
    Json(body): Json<CreateBindingRequest>,
) -> impl IntoResponse {
    let tid = match super::resolve_tenant_id(&state.platform_store, &tid_str) {
        Ok(id) => id,
        Err(e) => return e,
    };
    let tenant_store = match state.open_tenant_store(&tid) {
        Ok(s) => s,
        Err(e) => return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    let aid = match super::resolve_agent_id(&tenant_store, tid, &aid_str) {
        Ok(id) => id,
        Err(e) => return e,
    };
    let conn = tenant_store.conn();
    match ChatSurfaceBinding::create(conn, tid, aid, &body.surface_type, &body.surface_ref) {
        Ok(binding) => (
            StatusCode::CREATED,
            Json(serde_json::to_value(binding).unwrap()),
        ),
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

pub async fn list_bindings(
    State(state): State<Arc<super::super::AppState>>,
    Path((tid_str, aid_str)): Path<(String, String)>,
) -> impl IntoResponse {
    let tid = match super::resolve_tenant_id(&state.platform_store, &tid_str) {
        Ok(id) => id,
        Err(e) => return e,
    };
    let tenant_store = match state.open_tenant_store(&tid) {
        Ok(s) => s,
        Err(e) => return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    let aid = match super::resolve_agent_id(&tenant_store, tid, &aid_str) {
        Ok(id) => id,
        Err(e) => return e,
    };
    let conn = tenant_store.conn();
    match ChatSurfaceBinding::list_by_agent(conn, tid, aid) {
        Ok(bindings) => (
            StatusCode::OK,
            Json(serde_json::to_value(bindings).unwrap()),
        ),
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

pub async fn delete_binding(
    State(state): State<Arc<super::super::AppState>>,
    Path((tid_str, aid_str, bid)): Path<(String, String, uuid::Uuid)>,
) -> impl IntoResponse {
    let tid = match super::resolve_tenant_id(&state.platform_store, &tid_str) {
        Ok(id) => id,
        Err(e) => return e,
    };
    let tenant_store = match state.open_tenant_store(&tid) {
        Ok(s) => s,
        Err(e) => return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    let _aid = match super::resolve_agent_id(&tenant_store, tid, &aid_str) {
        Ok(id) => id,
        Err(e) => return e,
    };
    let conn = tenant_store.conn();
    match ChatSurfaceBinding::delete(conn, bid) {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({ "deleted": true }))),
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}
