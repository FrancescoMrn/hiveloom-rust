use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::engine::agent_loop::{AgentInvocation, InvocationResult};
use crate::store::models::{Agent, Capability, Conversation, ConversationTurn, MemoryEntry};

/// JSON-RPC 2.0 request structure.
#[derive(Debug, Clone, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub method: String,
    pub params: Option<serde_json::Value>,
    pub id: Option<serde_json::Value>,
}

/// JSON-RPC 2.0 response structure.
#[derive(Debug, Serialize, Clone)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
    pub id: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Clone)]
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

const _PARSE_ERROR: i32 = -32700;
const INVALID_REQUEST: i32 = -32600;
const METHOD_NOT_FOUND: i32 = -32601;
const INVALID_PARAMS: i32 = -32602;
const INTERNAL_ERROR: i32 = -32603;

/// POST /mcp/:tenant_slug/:agent_slug
///
/// Streamable HTTP transport for MCP (T082). The chat tool returns SSE;
/// other tools return a plain JSON-RPC response. All synchronous DB access
/// runs inside `spawn_blocking` to avoid holding `MutexGuard` across awaits.
pub async fn handle_mcp_request(
    State(state): State<Arc<crate::server::AppState>>,
    Path(params): Path<(String, String)>,
    headers: HeaderMap,
    Json(rpc_req): Json<JsonRpcRequest>,
) -> Result<Response, StatusCode> {
    let (tenant_slug, agent_slug) = params;

    if rpc_req.jsonrpc != "2.0" {
        return Ok(Json(JsonRpcResponse::error(
            rpc_req.id,
            INVALID_REQUEST,
            "Invalid JSON-RPC version".to_string(),
        ))
        .into_response());
    }

    // Handle stateless methods that don't need DB
    if rpc_req.method == "ping" {
        return Ok(
            Json(JsonRpcResponse::success(rpc_req.id, serde_json::json!({}))).into_response(),
        );
    }

    // Check if this is a chat tool call that needs SSE streaming
    let is_chat_tool = rpc_req.method == "tools/call"
        && rpc_req
            .params
            .as_ref()
            .and_then(|p| p.get("name"))
            .and_then(|v| v.as_str())
            == Some("chat");

    if is_chat_tool {
        return handle_chat_sse(state, &tenant_slug, &agent_slug, &headers, &rpc_req).await;
    }

    // All other methods: synchronous DB work in spawn_blocking
    let state_clone = Arc::clone(&state);
    let rpc_clone = rpc_req.clone();
    let access_token = bearer_token(&headers).map(str::to_string);

    let response = tokio::task::spawn_blocking(move || {
        handle_mcp_sync(
            &state_clone,
            &tenant_slug,
            &agent_slug,
            access_token.as_deref(),
            &rpc_clone,
        )
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)??;

    Ok(Json(response).into_response())
}

/// Synchronous handler — runs on the blocking threadpool so MutexGuard is safe.
fn handle_mcp_sync(
    state: &crate::server::AppState,
    tenant_slug: &str,
    agent_slug: &str,
    access_token: Option<&str>,
    rpc_req: &JsonRpcRequest,
) -> Result<JsonRpcResponse, StatusCode> {
    // Resolve tenant
    let tenant = {
        let conn = state.platform_store.conn();
        crate::store::models::Tenant::get_by_slug(&conn, tenant_slug)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .ok_or(StatusCode::NOT_FOUND)?
    };

    let tenant_store = state
        .open_tenant_store(&tenant.id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let conn = tenant_store.conn();
    let session = match access_token {
        Some(token) => crate::server::mcp::session::validate_session(conn, token)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        None => None,
    }
    .ok_or(StatusCode::UNAUTHORIZED)?;

    // Resolve agent
    let agent = resolve_agent_by_slug(conn, tenant.id, agent_slug)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    // Enforce agent binding
    if let Some(bound_agent_id) = session.identity.agent_id {
        if bound_agent_id != agent.id {
            return Err(StatusCode::FORBIDDEN);
        }
    }

    // Dispatch based on method
    let response = match rpc_req.method.as_str() {
        "initialize" => handle_initialize(&agent, rpc_req.id.clone()),

        "tools/list" => handle_tools_list(rpc_req.id.clone()),

        "tools/call" => handle_tools_call_sync(
            conn,
            &agent,
            &session.user_identity,
            rpc_req.params.clone(),
            rpc_req.id.clone(),
        ),

        _ => JsonRpcResponse::error(
            rpc_req.id.clone(),
            METHOD_NOT_FOUND,
            format!("Method not found: {}", rpc_req.method),
        ),
    };

    Ok(response)
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let raw = headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?;
    raw.strip_prefix("Bearer ")
        .or_else(|| raw.strip_prefix("bearer "))
        .map(str::trim)
}

fn resolve_agent_by_slug(
    conn: &rusqlite::Connection,
    tenant_id: uuid::Uuid,
    agent_slug: &str,
) -> anyhow::Result<Option<Agent>> {
    let agents = Agent::list_current(conn, tenant_id)?;
    Ok(agents.into_iter().find(|a| {
        a.name.to_lowercase().replace(' ', "-") == agent_slug.to_lowercase()
            || a.name.to_lowercase() == agent_slug.to_lowercase()
    }))
}

fn handle_initialize(agent: &Agent, id: Option<serde_json::Value>) -> JsonRpcResponse {
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

// ── tools/list ──────────────────────────────────────────────────────────

/// Returns the static set of platform-level MCP tools (chat, memory, list_conversations).
/// Individual agent capabilities are NOT exposed — they are internal to the agent loop.
fn handle_tools_list(id: Option<serde_json::Value>) -> JsonRpcResponse {
    JsonRpcResponse::success(
        id,
        serde_json::json!({
            "tools": [
                {
                    "name": "chat",
                    "description": "Send a message to the agent and receive a conversational response. The agent will use its configured capabilities internally as needed.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "message": {
                                "type": "string",
                                "description": "Your message to the agent"
                            },
                            "conversation_id": {
                                "type": "string",
                                "description": "Optional. ID of an existing conversation to continue. Omit to start a new conversation."
                            }
                        },
                        "required": ["message"]
                    }
                },
                {
                    "name": "memory",
                    "description": "Search your stored memory entries with this agent. Returns memories matching your query.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "query": {
                                "type": "string",
                                "description": "Search text to match against memory keys and values"
                            }
                        },
                        "required": ["query"]
                    }
                },
                {
                    "name": "list_conversations",
                    "description": "List your recent conversations with this agent. Returns conversation IDs, timestamps, and message previews for resuming prior conversations.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "limit": {
                                "type": "integer",
                                "description": "Maximum number of conversations to return. Default: 20.",
                                "default": 20
                            }
                        }
                    }
                }
            ]
        }),
    )
}

// ── tools/call dispatch (non-chat) ──────────────────────────────────────

/// Dispatch tools/call for non-chat tools (memory, list_conversations).
/// The chat tool is handled separately via SSE in handle_chat_sse.
fn handle_tools_call_sync(
    conn: &rusqlite::Connection,
    agent: &Agent,
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
            return JsonRpcResponse::error(id, INVALID_REQUEST, "Missing params.name".to_string());
        }
    };

    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or(serde_json::json!({}));

    match tool_name.as_str() {
        "memory" => handle_memory_tool(conn, agent, user_identity, &arguments, id),
        "list_conversations" => {
            handle_list_conversations_tool(conn, agent, user_identity, &arguments, id)
        }
        _ => JsonRpcResponse::error(
            id,
            METHOD_NOT_FOUND,
            format!("Tool not found: {}", tool_name),
        ),
    }
}

// ── chat tool (SSE) ─────────────────────────────────────────────────────

/// Handle the chat tool via SSE streaming. This runs the agent loop and
/// streams progress events followed by the final result.
async fn handle_chat_sse(
    state: Arc<crate::server::AppState>,
    tenant_slug: &str,
    agent_slug: &str,
    headers: &HeaderMap,
    rpc_req: &JsonRpcRequest,
) -> Result<Response, StatusCode> {
    let access_token = bearer_token(headers)
        .map(str::to_string)
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let tenant_slug = tenant_slug.to_string();
    let agent_slug = agent_slug.to_string();
    let rpc_id = rpc_req.id.clone();
    let params = rpc_req.params.clone().unwrap_or(serde_json::json!({}));

    // Extract arguments
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or(serde_json::json!({}));

    let message = match arguments.get("message").and_then(|v| v.as_str()) {
        Some(m) if !m.trim().is_empty() => m.to_string(),
        _ => {
            return Ok(Json(JsonRpcResponse::error(
                rpc_id,
                INVALID_PARAMS,
                "message parameter is required and must not be empty".to_string(),
            ))
            .into_response());
        }
    };

    let conversation_id_input = arguments
        .get("conversation_id")
        .and_then(|v| v.as_str())
        .and_then(|s| uuid::Uuid::parse_str(s).ok());

    // Run the entire agent loop in spawn_blocking, then stream the result as SSE
    let state_clone = Arc::clone(&state);
    let rpc_id_clone = rpc_id.clone();

    let chat_result: Result<(String, String, Vec<String>), (i32, String)> =
        tokio::task::spawn_blocking(move || {
            handle_chat_blocking(
                &state_clone,
                &tenant_slug,
                &agent_slug,
                &access_token,
                &message,
                conversation_id_input,
            )
        })
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Build SSE response with a single result event
    let rpc_id_for_stream = rpc_id_clone;
    let stream = futures::stream::once(async move {
        match chat_result {
            Ok((response_text, conv_id, capabilities_used)) => {
                let result = serde_json::json!({
                    "content": [{"type": "text", "text": response_text}],
                    "conversation_id": conv_id,
                    "capabilities_used": capabilities_used
                });
                let rpc_response = JsonRpcResponse::success(rpc_id_for_stream, result);
                Event::default()
                    .json_data(&rpc_response)
                    .map_err(std::io::Error::other)
            }
            Err((_code, err_msg)) => {
                // Return a graceful error as a chat response, not a JSON-RPC error
                let result = serde_json::json!({
                    "content": [{"type": "text", "text": "I'm sorry, I'm unable to process your request right now. Please try again shortly."}],
                    "conversation_id": "",
                    "capabilities_used": []
                });
                let rpc_response = JsonRpcResponse::success(rpc_id_for_stream, result);
                tracing::error!(error = %err_msg, "Chat tool error");
                Event::default()
                    .json_data(&rpc_response)
                    .map_err(std::io::Error::other)
            }
        }
    });

    Ok(Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response())
}

/// Blocking portion of the chat tool: resolves session, conversation, runs agent loop.
/// Returns (response_text, conversation_id, capabilities_used) or (error_code, error_message).
fn handle_chat_blocking(
    state: &crate::server::AppState,
    tenant_slug: &str,
    agent_slug: &str,
    access_token: &str,
    message: &str,
    conversation_id_input: Option<uuid::Uuid>,
) -> Result<(String, String, Vec<String>), (i32, String)> {
    // Resolve tenant
    let tenant = {
        let conn = state.platform_store.conn();
        crate::store::models::Tenant::get_by_slug(&conn, tenant_slug)
            .map_err(|e| (INTERNAL_ERROR, format!("Tenant resolution failed: {}", e)))?
            .ok_or_else(|| (INTERNAL_ERROR, "Tenant not found".to_string()))?
    };

    let tenant_store = state.open_tenant_store(&tenant.id).map_err(|e| {
        (
            INTERNAL_ERROR,
            format!("Failed to open tenant store: {}", e),
        )
    })?;
    let conn = tenant_store.conn();

    // Validate session
    let session = crate::server::mcp::session::validate_session(conn, access_token)
        .map_err(|e| (INTERNAL_ERROR, format!("Session validation failed: {}", e)))?
        .ok_or_else(|| (INTERNAL_ERROR, "Invalid session".to_string()))?;

    // Resolve agent
    let agent = resolve_agent_by_slug(conn, tenant.id, agent_slug)
        .map_err(|e| (INTERNAL_ERROR, format!("Agent resolution failed: {}", e)))?
        .ok_or_else(|| (INTERNAL_ERROR, "Agent not found".to_string()))?;

    // Enforce agent binding
    if let Some(bound_agent_id) = session.identity.agent_id {
        if bound_agent_id != agent.id {
            return Err((INTERNAL_ERROR, "Agent binding mismatch".to_string()));
        }
    }

    // Resolve or create conversation
    let conversation = resolve_conversation(
        conn,
        &tenant,
        &agent,
        &session.user_identity,
        conversation_id_input,
    )?;

    // Load capabilities for the agent loop
    let capabilities = Capability::list_by_agent(conn, agent.tenant_id, agent.id).map_err(|e| {
        (
            INTERNAL_ERROR,
            format!("Failed to load capabilities: {}", e),
        )
    })?;

    // Resolve LLM credential
    let credential_name = if agent.model_id.starts_with("claude-") {
        "anthropic"
    } else {
        "openai"
    };
    let api_key = {
        let entry = crate::store::models::CredentialVaultEntry::get_by_name(
            conn,
            tenant.id,
            credential_name,
            None,
        )
        .map_err(|e| {
            (
                INTERNAL_ERROR,
                format!("Failed to resolve LLM credential: {}", e),
            )
        })?
        .ok_or_else(|| {
            (
                INTERNAL_ERROR,
                format!("No LLM credential '{}' found for tenant", credential_name),
            )
        })?;
        let decrypted = state.vault.decrypt(&entry.encrypted_value).map_err(|e| {
            (
                INTERNAL_ERROR,
                format!("Failed to decrypt credential: {}", e),
            )
        })?;
        String::from_utf8(decrypted).map_err(|e| {
            (
                INTERNAL_ERROR,
                format!("Invalid credential encoding: {}", e),
            )
        })?
    };

    let provider = crate::llm::resolve_provider(&agent.model_id, &api_key).map_err(|e| {
        (
            INTERNAL_ERROR,
            format!("Failed to resolve LLM provider: {}", e),
        )
    })?;

    let invocation = AgentInvocation {
        agent: agent.clone(),
        capabilities,
        conversation_id: conversation.id,
        tenant_id: tenant.id,
        user_identity: session.user_identity.clone(),
    };

    // Run the agent loop (async, but we're in spawn_blocking)
    let rt = tokio::runtime::Handle::current();
    let result: InvocationResult = rt
        .block_on(crate::engine::agent_loop::run_agent_loop_with_vault(
            &invocation,
            provider.as_ref(),
            conn,
            message,
            &state.vault,
        ))
        .map_err(|e| (INTERNAL_ERROR, format!("Agent loop failed: {}", e)))?;

    Ok((
        result.response,
        conversation.id.to_string(),
        result.tool_calls_made,
    ))
}

/// Resolve or create a conversation for the chat tool.
fn resolve_conversation(
    conn: &rusqlite::Connection,
    tenant: &crate::store::models::Tenant,
    agent: &Agent,
    user_identity: &str,
    conversation_id_input: Option<uuid::Uuid>,
) -> Result<Conversation, (i32, String)> {
    if let Some(conv_id) = conversation_id_input {
        // Try to load the specified conversation with scoping
        if let Some(conv) =
            Conversation::get_by_id_scoped(conn, conv_id, tenant.id, agent.id, user_identity)
                .map_err(|e| (INTERNAL_ERROR, format!("Conversation lookup failed: {}", e)))?
        {
            return Ok(conv);
        }
        // Invalid/expired/wrong user — fall through to create new
    }

    // Create a new conversation
    let conv_uuid = uuid::Uuid::new_v4();
    let surface_ref = format!("mcp-chat:{}", conv_uuid);
    Conversation::create(
        conn,
        tenant.id,
        agent.id,
        "mcp",
        &surface_ref,
        user_identity,
        None,
    )
    .map_err(|e| {
        (
            INTERNAL_ERROR,
            format!("Failed to create conversation: {}", e),
        )
    })
}

// ── memory tool ─────────────────────────────────────────────────────────

fn handle_memory_tool(
    conn: &rusqlite::Connection,
    agent: &Agent,
    user_identity: &str,
    arguments: &serde_json::Value,
    id: Option<serde_json::Value>,
) -> JsonRpcResponse {
    let query = match arguments.get("query").and_then(|v| v.as_str()) {
        Some(q) => q,
        None => {
            return JsonRpcResponse::error(
                id,
                INVALID_PARAMS,
                "query parameter is required".to_string(),
            );
        }
    };

    let entries = match MemoryEntry::search_by_query(
        conn,
        agent.tenant_id,
        agent.id,
        user_identity,
        query,
        50,
    ) {
        Ok(e) => e,
        Err(e) => {
            return JsonRpcResponse::error(
                id,
                INTERNAL_ERROR,
                format!("Memory search failed: {}", e),
            );
        }
    };

    let results: Vec<serde_json::Value> = entries
        .iter()
        .map(|e| {
            let scope_display = if e.scope.starts_with("user:") {
                "user"
            } else {
                &e.scope
            };
            serde_json::json!({
                "key": e.key,
                "value": e.value,
                "scope": scope_display,
                "created_at": e.created_at
            })
        })
        .collect();

    let text = serde_json::to_string(&results).unwrap_or_else(|_| "[]".to_string());

    JsonRpcResponse::success(
        id,
        serde_json::json!({
            "content": [{"type": "text", "text": text}]
        }),
    )
}

// ── list_conversations tool ─────────────────────────────────────────────

fn handle_list_conversations_tool(
    conn: &rusqlite::Connection,
    agent: &Agent,
    user_identity: &str,
    arguments: &serde_json::Value,
    id: Option<serde_json::Value>,
) -> JsonRpcResponse {
    let limit = arguments
        .get("limit")
        .and_then(|v| v.as_i64())
        .unwrap_or(20);

    let conversations = match Conversation::list_by_user_and_agent(
        conn,
        agent.tenant_id,
        agent.id,
        user_identity,
        limit,
    ) {
        Ok(c) => c,
        Err(e) => {
            return JsonRpcResponse::error(
                id,
                INTERNAL_ERROR,
                format!("Failed to list conversations: {}", e),
            );
        }
    };

    let results: Vec<serde_json::Value> = conversations
        .iter()
        .map(|c| {
            // Get last assistant turn for preview
            let preview = ConversationTurn::get_last_assistant_turn(conn, c.id)
                .ok()
                .flatten()
                .map(|t| {
                    let content = t.content.clone();
                    if content.len() > 200 {
                        format!("{}...", &content[..197])
                    } else {
                        content
                    }
                })
                .unwrap_or_default();

            serde_json::json!({
                "conversation_id": c.id.to_string(),
                "status": c.status,
                "last_activity": c.updated_at,
                "preview": preview
            })
        })
        .collect();

    let text = serde_json::to_string(&results).unwrap_or_else(|_| "[]".to_string());

    JsonRpcResponse::success(
        id,
        serde_json::json!({
            "content": [{"type": "text", "text": text}]
        }),
    )
}
