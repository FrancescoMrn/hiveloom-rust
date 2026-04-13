use axum::{
    extract::{Json, Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::Deserialize;
use std::sync::Arc;

use crate::server::oauth::server as oauth_server;
use crate::store::models::{McpClientRegistration, McpIdentity, McpSetupCode};

fn err_json(status: StatusCode, msg: &str) -> (StatusCode, Json<serde_json::Value>) {
    (status, Json(serde_json::json!({ "error": msg })))
}

// ── CRUD ──────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateMcpIdentityRequest {
    pub name: String,
}

pub async fn create_mcp_identity(
    State(state): State<Arc<crate::server::AppState>>,
    Path(tid_str): Path<String>,
    Json(body): Json<CreateMcpIdentityRequest>,
) -> impl IntoResponse {
    let tenant_id = match super::resolve_tenant_id(&state.platform_store, &tid_str) {
        Ok(id) => id,
        Err(e) => return e.into_response(),
    };

    let tenant_store = match state.open_tenant_store(&tenant_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response()
        }
    };
    let conn = tenant_store.conn();

    match McpIdentity::create(conn, tenant_id, &body.name) {
        Ok(identity) => (
            StatusCode::CREATED,
            Json(serde_json::to_value(identity).unwrap()),
        )
            .into_response(),
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response(),
    }
}

pub async fn list_mcp_identities(
    State(state): State<Arc<crate::server::AppState>>,
    Path(tid_str): Path<String>,
) -> impl IntoResponse {
    let tenant_id = match super::resolve_tenant_id(&state.platform_store, &tid_str) {
        Ok(id) => id,
        Err(e) => return e.into_response(),
    };

    let tenant_store = match state.open_tenant_store(&tenant_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response()
        }
    };
    let conn = tenant_store.conn();

    match McpIdentity::list(conn, tenant_id) {
        Ok(identities) => Json(serde_json::to_value(identities).unwrap()).into_response(),
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response(),
    }
}

pub async fn get_mcp_identity(
    State(state): State<Arc<crate::server::AppState>>,
    Path((tid_str, mid)): Path<(String, String)>,
) -> impl IntoResponse {
    let tenant_id = match super::resolve_tenant_id(&state.platform_store, &tid_str) {
        Ok(id) => id,
        Err(e) => return e.into_response(),
    };
    let identity_id: uuid::Uuid = match mid.parse() {
        Ok(id) => id,
        Err(_) => return err_json(StatusCode::BAD_REQUEST, "Invalid identity id").into_response(),
    };

    let tenant_store = match state.open_tenant_store(&tenant_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response()
        }
    };
    let conn = tenant_store.conn();

    match McpIdentity::get(conn, identity_id) {
        Ok(Some(identity)) => Json(serde_json::to_value(identity).unwrap()).into_response(),
        Ok(None) => err_json(StatusCode::NOT_FOUND, "MCP identity not found").into_response(),
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response(),
    }
}

// ── Map / Unmap ───────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct MapPersonRequest {
    pub person_id: String,
}

pub async fn map_person(
    State(state): State<Arc<crate::server::AppState>>,
    Path((tid_str, mid)): Path<(String, String)>,
    Json(body): Json<MapPersonRequest>,
) -> impl IntoResponse {
    let tenant_id = match super::resolve_tenant_id(&state.platform_store, &tid_str) {
        Ok(id) => id,
        Err(e) => return e.into_response(),
    };
    let identity_id: uuid::Uuid = match mid.parse() {
        Ok(id) => id,
        Err(_) => return err_json(StatusCode::BAD_REQUEST, "Invalid identity id").into_response(),
    };

    let tenant_store = match state.open_tenant_store(&tenant_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response()
        }
    };
    let conn = tenant_store.conn();

    match McpIdentity::map_person(conn, identity_id, &body.person_id) {
        Ok(()) => Json(serde_json::json!({ "ok": true })).into_response(),
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response(),
    }
}

pub async fn unmap_person(
    State(state): State<Arc<crate::server::AppState>>,
    Path((tid_str, mid)): Path<(String, String)>,
) -> impl IntoResponse {
    let tenant_id = match super::resolve_tenant_id(&state.platform_store, &tid_str) {
        Ok(id) => id,
        Err(e) => return e.into_response(),
    };
    let identity_id: uuid::Uuid = match mid.parse() {
        Ok(id) => id,
        Err(_) => return err_json(StatusCode::BAD_REQUEST, "Invalid identity id").into_response(),
    };

    let tenant_store = match state.open_tenant_store(&tenant_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response()
        }
    };
    let conn = tenant_store.conn();

    match McpIdentity::unmap_person(conn, identity_id) {
        Ok(()) => Json(serde_json::json!({ "ok": true })).into_response(),
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response(),
    }
}

// ── Revoke ────────────────────────────────────────────────────────────

pub async fn revoke_mcp_identity(
    State(state): State<Arc<crate::server::AppState>>,
    Path((tid_str, mid)): Path<(String, String)>,
) -> impl IntoResponse {
    let tenant_id = match super::resolve_tenant_id(&state.platform_store, &tid_str) {
        Ok(id) => id,
        Err(e) => return e.into_response(),
    };
    let identity_id: uuid::Uuid = match mid.parse() {
        Ok(id) => id,
        Err(_) => return err_json(StatusCode::BAD_REQUEST, "Invalid identity id").into_response(),
    };

    let tenant_store = match state.open_tenant_store(&tenant_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response()
        }
    };
    let conn = tenant_store.conn();

    // Revoke identity and all its client registrations
    if let Err(e) = McpIdentity::revoke(conn, identity_id) {
        return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response();
    }

    // Revoke all client registrations for this identity
    let registrations = match McpClientRegistration::list_by_identity(conn, identity_id) {
        Ok(r) => r,
        Err(e) => {
            return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response()
        }
    };

    for reg in registrations {
        if reg.revoked_at.is_none() {
            let _ = McpClientRegistration::revoke(conn, reg.id);
        }
    }

    Json(serde_json::json!({ "ok": true })).into_response()
}

// ── Reissue setup code ───────────────────────────────────────────────

pub async fn reissue_setup_code(
    State(state): State<Arc<crate::server::AppState>>,
    Path((tid_str, mid)): Path<(String, String)>,
) -> impl IntoResponse {
    let tenant_id = match super::resolve_tenant_id(&state.platform_store, &tid_str) {
        Ok(id) => id,
        Err(e) => return e.into_response(),
    };
    let identity_id: uuid::Uuid = match mid.parse() {
        Ok(id) => id,
        Err(_) => return err_json(StatusCode::BAD_REQUEST, "Invalid identity id").into_response(),
    };

    let tenant_store = match state.open_tenant_store(&tenant_id) {
        Ok(s) => s,
        Err(e) => {
            return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response()
        }
    };
    let conn = tenant_store.conn();

    // Verify identity exists and is active
    match McpIdentity::get(conn, identity_id) {
        Ok(Some(id)) if id.status == "active" => {}
        Ok(Some(_)) => {
            return err_json(StatusCode::CONFLICT, "Identity is not active").into_response();
        }
        Ok(None) => {
            return err_json(StatusCode::NOT_FOUND, "MCP identity not found").into_response();
        }
        Err(e) => {
            return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response();
        }
    }

    // Revoke all existing unused setup codes
    if let Err(e) = McpSetupCode::revoke_all_for_identity(conn, identity_id) {
        return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response();
    }

    // Generate a new setup code
    let raw_code = oauth_server::generate_token();
    let code_hash = oauth_server::hash_token(&raw_code);
    let expires_at = (chrono::Utc::now() + chrono::Duration::hours(24)).to_rfc3339();

    match McpSetupCode::create(conn, tenant_id, identity_id, &code_hash, &expires_at) {
        Ok(_) => Json(serde_json::json!({
            "setup_code": raw_code,
            "expires_at": expires_at,
        }))
        .into_response(),
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response(),
    }
}
