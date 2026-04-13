use axum::{
    extract::{Json, Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::Deserialize;
use std::sync::Arc;

use crate::store::models::{Capability, CreateCapabilityParams, UpdateCapabilityParams};

fn err_json(status: StatusCode, msg: &str) -> (StatusCode, Json<serde_json::Value>) {
    (status, Json(serde_json::json!({ "error": msg })))
}

#[derive(Deserialize)]
pub struct CreateCapabilityRequest {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub endpoint_url: String,
    #[serde(default = "default_auth_type")]
    pub auth_type: String,
    pub credential_ref: Option<String>,
    pub input_schema: Option<String>,
    pub output_schema: Option<String>,
}

fn default_auth_type() -> String {
    "none".to_string()
}

#[derive(Deserialize)]
pub struct UpdateCapabilityRequest {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub endpoint_url: String,
    #[serde(default = "default_auth_type")]
    pub auth_type: String,
    pub credential_ref: Option<String>,
    pub input_schema: Option<String>,
    pub output_schema: Option<String>,
}

pub async fn create_capability(
    State(state): State<Arc<super::super::AppState>>,
    Path((tid_str, aid_str)): Path<(String, String)>,
    Json(body): Json<CreateCapabilityRequest>,
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
    match Capability::create(
        conn,
        CreateCapabilityParams {
            tenant_id: tid,
            agent_id: aid,
            name: &body.name,
            description: &body.description,
            endpoint_url: &body.endpoint_url,
            auth_type: &body.auth_type,
            credential_ref: body.credential_ref.as_deref(),
            input_schema: body.input_schema.as_deref(),
            output_schema: body.output_schema.as_deref(),
        },
    ) {
        Ok(cap) => (
            StatusCode::CREATED,
            Json(serde_json::to_value(cap).unwrap()),
        ),
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

pub async fn list_capabilities(
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
    match Capability::list_by_agent(conn, tid, aid) {
        Ok(caps) => (StatusCode::OK, Json(serde_json::to_value(caps).unwrap())),
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

pub async fn get_capability(
    State(state): State<Arc<super::super::AppState>>,
    Path((tid_str, aid_str, cid)): Path<(String, String, uuid::Uuid)>,
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
    match Capability::get(conn, cid) {
        Ok(Some(cap)) => (StatusCode::OK, Json(serde_json::to_value(cap).unwrap())),
        Ok(None) => err_json(StatusCode::NOT_FOUND, "Capability not found"),
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

pub async fn update_capability(
    State(state): State<Arc<super::super::AppState>>,
    Path((tid_str, aid_str, cid)): Path<(String, String, uuid::Uuid)>,
    Json(body): Json<UpdateCapabilityRequest>,
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
    match Capability::update(
        conn,
        UpdateCapabilityParams {
            id: cid,
            name: &body.name,
            description: &body.description,
            endpoint_url: &body.endpoint_url,
            auth_type: &body.auth_type,
            credential_ref: body.credential_ref.as_deref(),
            input_schema: body.input_schema.as_deref(),
            output_schema: body.output_schema.as_deref(),
        },
    ) {
        Ok(()) => match Capability::get(conn, cid) {
            Ok(Some(cap)) => (StatusCode::OK, Json(serde_json::to_value(cap).unwrap())),
            _ => err_json(StatusCode::NOT_FOUND, "Capability not found after update"),
        },
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

pub async fn delete_capability(
    State(state): State<Arc<super::super::AppState>>,
    Path((tid_str, aid_str, cid)): Path<(String, String, uuid::Uuid)>,
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
    match Capability::delete(conn, cid) {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({ "deleted": true }))),
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}
