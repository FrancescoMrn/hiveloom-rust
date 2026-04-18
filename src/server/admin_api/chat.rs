use axum::{
    extract::{Json, Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::Deserialize;
use std::sync::Arc;

use crate::engine::agent_loop::{AgentInvocation, InvocationResult};
use crate::store::models::{Agent, Capability, Conversation, CredentialVaultEntry};

fn err_json(status: StatusCode, msg: &str) -> (StatusCode, Json<serde_json::Value>) {
    (status, Json(serde_json::json!({ "error": msg })))
}

#[derive(Deserialize)]
pub struct ChatRequest {
    pub message: String,
    pub conversation_id: Option<String>,
}

/// POST /api/tenants/:tid/agents/:aid/chat
///
/// Admin-level chat with an agent. Used by the CLI `chat` command and interactive mode.
pub async fn chat_with_agent(
    State(state): State<Arc<crate::server::AppState>>,
    Path((tid_str, aid_str)): Path<(String, String)>,
    Json(body): Json<ChatRequest>,
) -> impl IntoResponse {
    let tid = match super::resolve_tenant_id(&state.platform_store, &tid_str) {
        Ok(id) => id,
        Err(e) => return e.into_response(),
    };
    let tenant_store = match state.open_tenant_store(&tid) {
        Ok(s) => s,
        Err(e) => {
            return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response()
        }
    };
    let aid = match super::resolve_agent_id(&tenant_store, tid, &aid_str) {
        Ok(id) => id,
        Err(e) => return e.into_response(),
    };

    let conn = tenant_store.conn();

    // Load agent
    let agent = match Agent::get_current(conn, tid, aid) {
        Ok(Some(a)) => a,
        Ok(None) => return err_json(StatusCode::NOT_FOUND, "Agent not found").into_response(),
        Err(e) => {
            return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response()
        }
    };

    // Load capabilities
    let capabilities = match Capability::list_by_agent(conn, tid, aid) {
        Ok(c) => c,
        Err(e) => {
            return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response()
        }
    };

    // Resolve LLM credential
    let credential_name = if agent.model_id.starts_with("claude-") {
        "anthropic"
    } else {
        "openai"
    };
    let api_key = match CredentialVaultEntry::get_by_name(conn, tid, credential_name, None) {
        Ok(Some(entry)) => match state.vault.decrypt(&entry.encrypted_value) {
            Ok(decrypted) => match String::from_utf8(decrypted) {
                Ok(key) => key,
                Err(e) => {
                    return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string())
                        .into_response()
                }
            },
            Err(e) => {
                return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response()
            }
        },
        Ok(None) => {
            let msg = format!(
                "No LLM credential '{}' found. Store one with: hiveloom credential set {} --from-env {}_API_KEY",
                credential_name,
                credential_name,
                credential_name.to_uppercase()
            );
            return err_json(StatusCode::BAD_REQUEST, &msg).into_response();
        }
        Err(e) => {
            return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response()
        }
    };

    let provider = match crate::llm::resolve_provider(&agent.model_id, &api_key) {
        Ok(p) => p,
        Err(e) => {
            return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response()
        }
    };

    // Resolve or create conversation
    let conversation =
        match resolve_cli_conversation(conn, tid, aid, "admin", body.conversation_id.as_deref()) {
            Ok(c) => c,
            Err(e) => {
                return err_json(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()).into_response()
            }
        };

    let invocation = AgentInvocation {
        agent: agent.clone(),
        capabilities,
        conversation_id: conversation.id,
        tenant_id: tid,
        user_identity: "admin".to_string(),
    };

    // Run agent loop in spawn_blocking (rusqlite Connection is !Send)
    let state_clone = Arc::clone(&state);
    let message = body.message.clone();

    let result = tokio::task::spawn_blocking(move || -> anyhow::Result<InvocationResult> {
        let tenant_store = state_clone.open_tenant_store(&tid)?;
        let conn = tenant_store.conn();
        let rt = tokio::runtime::Handle::current();
        rt.block_on(crate::engine::agent_loop::run_agent_loop_with_vault(
            &invocation,
            provider.as_ref(),
            conn,
            &message,
            &state_clone.vault,
        ))
    })
    .await;

    match result {
        Ok(Ok(inv_result)) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "response": inv_result.response,
                "conversation_id": conversation.id.to_string(),
                "capabilities_used": inv_result.tool_calls_made,
            })),
        )
            .into_response(),
        Ok(Err(e)) => {
            tracing::error!(error = %e, "Agent loop failed in admin chat");
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "response": "I'm sorry, I'm unable to process your request right now. Please try again shortly.",
                    "conversation_id": conversation.id.to_string(),
                    "capabilities_used": [],
                })),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "spawn_blocking failed in admin chat");
            err_json(StatusCode::INTERNAL_SERVER_ERROR, "Internal error").into_response()
        }
    }
}

fn resolve_cli_conversation(
    conn: &rusqlite::Connection,
    tenant_id: uuid::Uuid,
    agent_id: uuid::Uuid,
    user_identity: &str,
    conversation_id_str: Option<&str>,
) -> anyhow::Result<Conversation> {
    if let Some(cid_str) = conversation_id_str {
        if let Ok(cid) = uuid::Uuid::parse_str(cid_str) {
            if let Some(conv) =
                Conversation::get_by_id_scoped(conn, cid, tenant_id, agent_id, user_identity)?
            {
                return Ok(conv);
            }
        }
    }
    // Create new conversation
    let conv_uuid = uuid::Uuid::new_v4();
    let surface_ref = format!("cli-chat:{}", conv_uuid);
    Conversation::create(
        conn,
        tenant_id,
        agent_id,
        "mcp", // reuse mcp surface type for CLI conversations
        &surface_ref,
        user_identity,
        None,
    )
}
