use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::sync::Arc;

use crate::store::models::PlatformAdminToken;

#[derive(Debug, Deserialize)]
pub struct CreateTokenRequest {
    pub scope: Option<String>,
    pub expires_at: Option<String>,
}

/// POST /api/auth/tokens — create a new admin token
pub async fn create_token(
    State(state): State<Arc<crate::server::AppState>>,
    Json(req): Json<CreateTokenRequest>,
) -> impl IntoResponse {
    let scope = req.scope.as_deref().unwrap_or("platform:admin");

    // Generate a random token
    use rand::RngCore;
    let mut token_bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut token_bytes);
    let token_plain = hex::encode(token_bytes);
    let token_hash = hex::encode(Sha256::digest(token_plain.as_bytes()));

    let conn = state.platform_store.conn();
    match PlatformAdminToken::create(&conn, &token_hash, scope, req.expires_at.as_deref()) {
        Ok(admin_token) => {
            let response = serde_json::json!({
                "id": admin_token.id.to_string(),
                "token": token_plain,
                "scope": admin_token.scope,
                "created_at": admin_token.created_at,
                "expires_at": admin_token.expires_at,
            });
            (StatusCode::CREATED, Json(response))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ),
    }
}

/// GET /api/auth/tokens — list all tokens
pub async fn list_tokens(State(state): State<Arc<crate::server::AppState>>) -> impl IntoResponse {
    let conn = state.platform_store.conn();
    match PlatformAdminToken::list(&conn) {
        Ok(tokens) => {
            let result: Vec<serde_json::Value> = tokens
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "id": t.id.to_string(),
                        "scope": t.scope,
                        "created_at": t.created_at,
                        "expires_at": t.expires_at,
                        "revoked_at": t.revoked_at,
                    })
                })
                .collect();
            Json(serde_json::to_value(&result).unwrap())
        }
        Err(_) => Json(serde_json::json!([])),
    }
}

/// DELETE /api/auth/tokens/{id} — revoke a token
pub async fn revoke_token(
    State(state): State<Arc<crate::server::AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let token_id: uuid::Uuid = match id.parse() {
        Ok(id) => id,
        Err(_) => return StatusCode::BAD_REQUEST,
    };

    let conn = state.platform_store.conn();
    match PlatformAdminToken::revoke(&conn, token_id) {
        Ok(_) => StatusCode::NO_CONTENT,
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR,
    }
}
