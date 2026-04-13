use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::session;
use crate::store::models::{Agent, ChatSurfaceBinding};

/// JSON-RPC 2.0 request structure.
#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub method: String,
    pub params: Option<serde_json::Value>,
    pub id: Option<serde_json::Value>,
}

/// JSON-RPC 2.0 response structure.
#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
    pub id: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl JsonRpcResponse {
    pub fn success(id: Option<serde_json::Value>, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            result: Some(result),
            error: None,
            id,
        }
    }

    pub fn error(id: Option<serde_json::Value>, code: i32, message: String) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            result: None,
            error: Some(JsonRpcError {
                code,
                message,
                data: None,
            }),
            id,
        }
    }
}

// Standard JSON-RPC error codes
const PARSE_ERROR: i32 = -32700;
const INVALID_REQUEST: i32 = -32600;
const METHOD_NOT_FOUND: i32 = -32601;
const INTERNAL_ERROR: i32 = -32603;

/// POST /mcp/:tenant_slug/:agent_slug
///
/// Streamable HTTP transport for MCP (T082). Accepts a JSON-RPC request,
/// authenticates via Bearer token, resolves tenant + agent, dispatches
/// to the agent loop, and returns a JSON-RPC response.
pub async fn handle_mcp_request(
    State(state): State<Arc<crate::server::AppState>>,
    Path(params): Path<(String, String)>,
    Json(rpc_req): Json<JsonRpcRequest>,
) -> Result<Json<JsonRpcResponse>, StatusCode> {
    let (tenant_slug, agent_slug) = params;
    let headers = HeaderMap::new(); // TODO: extract from request if needed for auth
    // Validate JSON-RPC version
    if rpc_req.jsonrpc != "2.0" {
        return Ok(Json(JsonRpcResponse::error(
            rpc_req.id,
            INVALID_REQUEST,
            "Invalid JSON-RPC version".to_string(),
        )));
    }

    // Authenticate via Bearer token
    let bearer_token = extract_bearer_token(&headers).ok_or(StatusCode::UNAUTHORIZED)?;

    // Resolve tenant
    let tenant = {
        let conn = state.platform_store.conn();
        crate::store::models::Tenant::get_by_slug(&conn, &tenant_slug)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .ok_or(StatusCode::NOT_FOUND)?
    };

    let tenant_store = state
        .open_tenant_store(&tenant.id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let conn = tenant_store.conn();

    // Validate MCP session and resolve identity
    let mcp_session = session::validate_session(conn, &bearer_token)
        .map_err(|_| StatusCode::UNAUTHORIZED)?
        .ok_or(StatusCode::UNAUTHORIZED)?;

    // Resolve agent by name/slug from binding
    let agent = resolve_agent_by_slug(conn, tenant.id, &agent_slug)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    // Dispatch based on JSON-RPC method
    let response = match rpc_req.method.as_str() {
        "initialize" => handle_initialize(&agent, rpc_req.id),

        "tools/list" => handle_tools_list(conn, &agent, rpc_req.id),

        "tools/call" => {
            handle_tools_call(
                conn,
                &state,
                &agent,
                &tenant,
                &mcp_session.user_identity,
                rpc_req.params,
                rpc_req.id,
            )
            .await
        }

        "ping" => JsonRpcResponse::success(rpc_req.id, serde_json::json!({})),

        _ => JsonRpcResponse::error(
            rpc_req.id,
            METHOD_NOT_FOUND,
            format!("Method not found: {}", rpc_req.method),
        ),
    };

    Ok(Json(response))
}

fn extract_bearer_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(|s| s.to_string())
}

fn resolve_agent_by_slug(
    conn: &rusqlite::Connection,
    tenant_id: uuid::Uuid,
    agent_slug: &str,
) -> anyhow::Result<Option<Agent>> {
    // Try to find an agent by name matching the slug
    let agents = Agent::list_current(conn, tenant_id)?;
    Ok(agents.into_iter().find(|a| {
        a.name.to_lowercase().replace(' ', "-") == agent_slug.to_lowercase()
            || a.name.to_lowercase() == agent_slug.to_lowercase()
    }))
}

fn handle_initialize(
    agent: &Agent,
    id: Option<serde_json::Value>,
) -> JsonRpcResponse {
    JsonRpcResponse::success(
        id,
        serde_json::json!({
            "protocolVersion": "2025-03-26",
            "capabilities": {
                "tools": { "listChanged": false }
            },
            "serverInfo": {
                "name": format!("hiveloom-{}", agent.name),
                "version": env!("CARGO_PKG_VERSION")
            }
        }),
    )
}

fn handle_tools_list(
    conn: &rusqlite::Connection,
    agent: &Agent,
    id: Option<serde_json::Value>,
) -> JsonRpcResponse {
    let capabilities = match crate::store::models::Capability::list_by_agent(conn, agent.tenant_id, agent.id) {
        Ok(caps) => caps,
        Err(e) => {
            return JsonRpcResponse::error(
                id,
                INTERNAL_ERROR,
                format!("Failed to list tools: {}", e),
            );
        }
    };

    let tools: Vec<serde_json::Value> = capabilities
        .iter()
        .map(|c| {
            serde_json::json!({
                "name": c.name,
                "description": c.description,
                "inputSchema": c.input_schema
                    .as_ref()
                    .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
                    .unwrap_or(serde_json::json!({"type": "object"}))
            })
        })
        .collect();

    JsonRpcResponse::success(id, serde_json::json!({ "tools": tools }))
}

#[allow(clippy::too_many_arguments)]
async fn handle_tools_call(
    conn: &rusqlite::Connection,
    state: &Arc<crate::server::AppState>,
    agent: &Agent,
    tenant: &crate::store::models::Tenant,
    user_identity: &str,
    params: Option<serde_json::Value>,
    id: Option<serde_json::Value>,
) -> JsonRpcResponse {
    let params = match params {
        Some(p) => p,
        None => {
            return JsonRpcResponse::error(id, INVALID_REQUEST, "Missing params".to_string());
        }
    };

    let tool_name = match params.get("name").and_then(|v| v.as_str()) {
        Some(n) => n.to_string(),
        None => {
            return JsonRpcResponse::error(
                id,
                INVALID_REQUEST,
                "Missing params.name".to_string(),
            );
        }
    };

    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or(serde_json::json!({}));

    // Find matching capability
    let capabilities = match crate::store::models::Capability::list_by_agent(conn, agent.tenant_id, agent.id) {
        Ok(caps) => caps,
        Err(e) => {
            return JsonRpcResponse::error(
                id,
                INTERNAL_ERROR,
                format!("Failed to load capabilities: {}", e),
            );
        }
    };

    let capability = match capabilities.iter().find(|c| c.name == tool_name) {
        Some(c) => c,
        None => {
            return JsonRpcResponse::error(
                id,
                METHOD_NOT_FOUND,
                format!("Tool not found: {}", tool_name),
            );
        }
    };

    // Get or create conversation for this MCP session
    let conversation = match crate::engine::conversation::get_or_create_conversation(
        conn,
        &tenant.id,
        &agent.id,
        "mcp",
        &format!("mcp:{}", tenant.slug),
        user_identity,
        None,
    ) {
        Ok(c) => c,
        Err(e) => {
            return JsonRpcResponse::error(
                id,
                INTERNAL_ERROR,
                format!("Failed to create conversation: {}", e),
            );
        }
    };

    // Execute the capability
    match crate::engine::capability_exec::execute_capability(
        conn,
        capability,
        &arguments,
        &tenant.id,
        &agent.id,
        &conversation.id,
        &state.vault,
    )
    .await
    {
        Ok(result) => JsonRpcResponse::success(
            id,
            serde_json::json!({
                "content": [{
                    "type": "text",
                    "text": serde_json::to_string(&result).unwrap_or_default()
                }]
            }),
        ),
        Err(e) => JsonRpcResponse::error(id, INTERNAL_ERROR, format!("Tool execution failed: {}", e)),
    }
}
