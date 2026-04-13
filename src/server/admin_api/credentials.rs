use axum::{
    extract::{Json, Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::store::models::CredentialVaultEntry;

fn err_json(status: StatusCode, msg: &str) -> (StatusCode, Json<serde_json::Value>) {
    (status, Json(serde_json::json!({ "error": msg })))
}

#[derive(Deserialize)]
pub struct SetCredentialRequest {
    pub name: String,
    pub value: String,
    #[serde(default = "default_kind")]
    pub kind: String,
    pub agent_id: Option<uuid::Uuid>,
    pub provider: Option<String>,
    pub user_identity: Option<String>,
    pub granted_scopes: Option<String>,
}

fn default_kind() -> String {
    "static".to_string()
}

/// Summary of a credential (never includes the encrypted value).
#[derive(Serialize)]
pub struct CredentialSummary {
    pub id: uuid::Uuid,
    pub tenant_id: uuid::Uuid,
    pub agent_id: Option<uuid::Uuid>,
    pub name: String,
    pub kind: String,
    pub provider: Option<String>,
    pub user_identity: Option<String>,
    pub granted_scopes: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub rotated_at: Option<String>,
}

impl From<CredentialVaultEntry> for CredentialSummary {
    fn from(e: CredentialVaultEntry) -> Self {
        Self {
            id: e.id,
            tenant_id: e.tenant_id,
            agent_id: e.agent_id,
            name: e.name,
            kind: e.kind,
            provider: e.provider,
            user_identity: e.user_identity,
            granted_scopes: e.granted_scopes,
            created_at: e.created_at,
            updated_at: e.updated_at,
            rotated_at: e.rotated_at,
        }
    }
}

#[derive(Deserialize)]
pub struct RotateCredentialRequest {
    pub value: String,
}

pub async fn set_credential(
    State(state): State<Arc<super::super::AppState>>,
    Path(tid_str): Path<String>,
    Json(body): Json<SetCredentialRequest>,
) -> impl IntoResponse {
    let tid = match super::resolve_tenant_id(&state.platform_store, &tid_str) {
        Ok(id) => id,
        Err(e) => return e,
    };
    // Encrypt the value
    let encrypted = match state.vault.encrypt(body.value.as_bytes()) {
        Ok(e) => e,
        Err(e) => return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };

    let tenant_store = match state.open_tenant_store(&tid) {
        Ok(s) => s,
        Err(e) => return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    let conn = tenant_store.conn();

    match CredentialVaultEntry::create(
        conn,
        tid,
        body.agent_id,
        &body.name,
        &body.kind,
        &encrypted,
        body.provider.as_deref(),
        body.user_identity.as_deref(),
        body.granted_scopes.as_deref(),
    ) {
        Ok(entry) => {
            let summary: CredentialSummary = entry.into();
            (
                StatusCode::CREATED,
                Json(serde_json::to_value(summary).unwrap()),
            )
        }
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

pub async fn list_credentials(
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
    match CredentialVaultEntry::list(conn, tid) {
        Ok(entries) => {
            let summaries: Vec<CredentialSummary> = entries.into_iter().map(|e| e.into()).collect();
            (
                StatusCode::OK,
                Json(serde_json::to_value(summaries).unwrap()),
            )
        }
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

pub async fn delete_credential(
    State(state): State<Arc<super::super::AppState>>,
    Path((tid_str, name)): Path<(String, String)>,
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

    // Find credential by name
    match CredentialVaultEntry::get_by_name(conn, tid, &name, None) {
        Ok(Some(entry)) => match CredentialVaultEntry::delete(conn, entry.id) {
            Ok(()) => (StatusCode::OK, Json(serde_json::json!({ "deleted": true }))),
            Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
        },
        Ok(None) => err_json(StatusCode::NOT_FOUND, "Credential not found"),
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

pub async fn rotate_credential(
    State(state): State<Arc<super::super::AppState>>,
    Path((tid_str, name)): Path<(String, String)>,
    Json(body): Json<RotateCredentialRequest>,
) -> impl IntoResponse {
    let tid = match super::resolve_tenant_id(&state.platform_store, &tid_str) {
        Ok(id) => id,
        Err(e) => return e,
    };
    // Encrypt new value
    let encrypted = match state.vault.encrypt(body.value.as_bytes()) {
        Ok(e) => e,
        Err(e) => return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };

    let tenant_store = match state.open_tenant_store(&tid) {
        Ok(s) => s,
        Err(e) => return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    };
    let conn = tenant_store.conn();

    match CredentialVaultEntry::get_by_name(conn, tid, &name, None) {
        Ok(Some(entry)) => {
            match CredentialVaultEntry::update_encrypted_value(conn, entry.id, &encrypted) {
                Ok(()) => (StatusCode::OK, Json(serde_json::json!({ "rotated": true }))),
                Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
            }
        }
        Ok(None) => err_json(StatusCode::NOT_FOUND, "Credential not found"),
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}
