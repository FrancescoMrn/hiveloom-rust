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
    #[serde(default)]
    pub endpoint_url: Option<String>,
    #[serde(default = "default_auth_type")]
    pub auth_type: String,
    pub credential_ref: Option<String>,
    pub input_schema: Option<String>,
    pub output_schema: Option<String>,
    pub instruction_content: Option<String>,
}

fn default_auth_type() -> String {
    "none".to_string()
}

fn resolve_capability(
    conn: &rusqlite::Connection,
    tenant_id: uuid::Uuid,
    agent_id: uuid::Uuid,
    cid_or_name: &str,
) -> Result<Capability, (StatusCode, Json<serde_json::Value>)> {
    let caps = Capability::list_by_agent(conn, tenant_id, agent_id)
        .map_err(|e| err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()))?;
    let parsed_id = uuid::Uuid::parse_str(cid_or_name).ok();
    caps.into_iter()
        .find(|cap| parsed_id == Some(cap.id) || cap.name.eq_ignore_ascii_case(cid_or_name))
        .ok_or_else(|| err_json(StatusCode::NOT_FOUND, "Capability not found"))
}

#[derive(Deserialize)]
pub struct UpdateCapabilityRequest {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub endpoint_url: Option<String>,
    #[serde(default = "default_auth_type")]
    pub auth_type: String,
    pub credential_ref: Option<String>,
    pub input_schema: Option<String>,
    pub output_schema: Option<String>,
    pub instruction_content: Option<String>,
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
    let endpoint_url = body.endpoint_url.as_deref().unwrap_or("");
    match Capability::create(
        conn,
        CreateCapabilityParams {
            tenant_id: tid,
            agent_id: aid,
            name: &body.name,
            description: &body.description,
            endpoint_url,
            auth_type: &body.auth_type,
            credential_ref: body.credential_ref.as_deref(),
            input_schema: body.input_schema.as_deref(),
            output_schema: body.output_schema.as_deref(),
            instruction_content: body.instruction_content.as_deref(),
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
    Path((tid_str, aid_str, cid_or_name)): Path<(String, String, String)>,
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
    match resolve_capability(conn, tid, aid, &cid_or_name) {
        Ok(cap) => (StatusCode::OK, Json(serde_json::to_value(cap).unwrap())),
        Err(e) => e,
    }
}

pub async fn update_capability(
    State(state): State<Arc<super::super::AppState>>,
    Path((tid_str, aid_str, cid_or_name)): Path<(String, String, String)>,
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
    let aid = match super::resolve_agent_id(&tenant_store, tid, &aid_str) {
        Ok(id) => id,
        Err(e) => return e,
    };
    let conn = tenant_store.conn();
    let cap = match resolve_capability(conn, tid, aid, &cid_or_name) {
        Ok(cap) => cap,
        Err(e) => return e,
    };
    let endpoint_url = body.endpoint_url.as_deref().unwrap_or("");
    match Capability::update(
        conn,
        UpdateCapabilityParams {
            id: cap.id,
            name: &body.name,
            description: &body.description,
            endpoint_url,
            auth_type: &body.auth_type,
            credential_ref: body.credential_ref.as_deref(),
            input_schema: body.input_schema.as_deref(),
            output_schema: body.output_schema.as_deref(),
            instruction_content: body.instruction_content.as_deref(),
        },
    ) {
        Ok(()) => match Capability::get(conn, cap.id) {
            Ok(Some(cap)) => (StatusCode::OK, Json(serde_json::to_value(cap).unwrap())),
            _ => err_json(StatusCode::NOT_FOUND, "Capability not found after update"),
        },
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

pub async fn delete_capability(
    State(state): State<Arc<super::super::AppState>>,
    Path((tid_str, aid_str, cid_or_name)): Path<(String, String, String)>,
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
    let cap = match resolve_capability(conn, tid, aid, &cid_or_name) {
        Ok(cap) => cap,
        Err(e) => return e,
    };
    match Capability::delete(conn, cap.id) {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({ "deleted": true }))),
        Err(e) => err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::models::{Agent, Capability};

    #[test]
    fn resolves_capability_by_uuid_or_name_within_agent_scope() {
        let data_dir = tempfile::TempDir::new().unwrap();
        let tenant_id = uuid::Uuid::new_v4();
        let store = crate::store::TenantStore::open(data_dir.path(), &tenant_id).unwrap();
        let conn = store.conn();
        let agent = Agent::create(
            conn,
            tenant_id,
            "test-agent",
            "You are helpful.",
            "claude-test",
            "dual",
            "tenant",
            "coerce",
            false,
            None,
        )
        .unwrap();
        let other_agent = Agent::create(
            conn,
            tenant_id,
            "other-agent",
            "You are helpful.",
            "claude-test",
            "dual",
            "tenant",
            "coerce",
            false,
            None,
        )
        .unwrap();
        let cap = Capability::create(
            conn,
            CreateCapabilityParams {
                tenant_id,
                agent_id: agent.id,
                name: "SearchDocs",
                description: "Search docs",
                endpoint_url: "https://example.com/search",
                auth_type: "none",
                credential_ref: None,
                input_schema: None,
                output_schema: None,
                instruction_content: None,
            },
        )
        .unwrap();
        Capability::create(
            conn,
            CreateCapabilityParams {
                tenant_id,
                agent_id: other_agent.id,
                name: "SearchDocs",
                description: "Other search docs",
                endpoint_url: "https://example.com/other",
                auth_type: "none",
                credential_ref: None,
                input_schema: None,
                output_schema: None,
                instruction_content: None,
            },
        )
        .unwrap();

        assert_eq!(
            resolve_capability(conn, tenant_id, agent.id, &cap.id.to_string())
                .unwrap()
                .id,
            cap.id
        );
        assert_eq!(
            resolve_capability(conn, tenant_id, agent.id, "searchdocs")
                .unwrap()
                .id,
            cap.id
        );
        assert!(resolve_capability(conn, tenant_id, other_agent.id, &cap.id.to_string()).is_err());
    }
}
