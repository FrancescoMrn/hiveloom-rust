use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::server::oauth::server as oauth_server;
use crate::store::models::{McpClientRegistration, McpSetupCode};

// ── T085: Metadata endpoints ──────────────────────────────────────────

/// GET /.well-known/oauth-authorization-server
///
/// Returns the OAuth Authorization Server metadata per RFC 8414.
pub async fn oauth_metadata(
    State(_state): State<Arc<crate::server::AppState>>,
) -> impl IntoResponse {
    Json(serde_json::json!({
        "issuer": "https://hiveloom.local",
        "authorization_endpoint": "/mcp/authorize",
        "token_endpoint": "/mcp/token",
        "response_types_supported": ["code"],
        "grant_types_supported": ["authorization_code", "refresh_token"],
        "token_endpoint_auth_methods_supported": ["none"],
        "code_challenge_methods_supported": ["S256"]
    }))
}

/// GET /mcp/:tenant_slug/.well-known/oauth-protected-resource
///
/// Returns the protected resource metadata.
pub async fn protected_resource_metadata(
    Path(tenant_slug): Path<String>,
) -> impl IntoResponse {
    Json(serde_json::json!({
        "resource": format!("https://hiveloom.local/mcp/{}", tenant_slug),
        "authorization_servers": ["https://hiveloom.local"],
        "bearer_methods_supported": ["header"]
    }))
}

// ── T086: Authorize endpoint (setup code entry) ───────────────────────

#[derive(Debug, Deserialize)]
pub struct AuthorizeParams {
    pub response_type: String,
    pub client_id: Option<String>,
    pub state: Option<String>,
    pub redirect_uri: Option<String>,
    pub code_challenge: Option<String>,
    pub code_challenge_method: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SetupCodeSubmission {
    pub setup_code: String,
    pub tenant_slug: String,
    pub client_id: Option<String>,
    pub state: Option<String>,
    pub redirect_uri: Option<String>,
}

/// GET /mcp/authorize
///
/// Returns a setup code entry form. In production this would be an HTML
/// page; here we return JSON indicating that a setup code is required.
pub async fn authorize(
    Query(params): Query<AuthorizeParams>,
) -> impl IntoResponse {
    if params.response_type != "code" {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "unsupported_response_type"
            })),
        );
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "action": "enter_setup_code",
            "message": "Enter your MCP setup code to authorize this client.",
            "client_id": params.client_id,
            "state": params.state,
            "redirect_uri": params.redirect_uri
        })),
    )
}

/// POST /mcp/authorize
///
/// Submit a setup code to authorize. Validates the code, creates a
/// client registration, and returns an authorization code.
pub async fn authorize_submit(
    State(state): State<Arc<crate::server::AppState>>,
    Json(submission): Json<SetupCodeSubmission>,
) -> Result<impl IntoResponse, StatusCode> {
    // Resolve tenant
    let tenant = {
        let conn = state.platform_store.conn();
        crate::store::models::Tenant::get_by_slug(&conn, &submission.tenant_slug)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .ok_or(StatusCode::NOT_FOUND)?
    };

    let tenant_store = state
        .open_tenant_store(&tenant.id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let conn = tenant_store.conn();

    // Hash the submitted setup code and look it up
    let code_hash = oauth_server::hash_token(&submission.setup_code);
    let setup_code = McpSetupCode::get_valid_by_hash(conn, &code_hash)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::UNAUTHORIZED)?;

    // Mark the setup code as used
    McpSetupCode::mark_used(conn, setup_code.id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Generate an authorization code (short-lived, to be exchanged at token endpoint)
    let auth_code = oauth_server::generate_token();

    // Create client registration with the auth code as a temporary token.
    // The real access token will be issued at the token endpoint.
    let client_id = submission
        .client_id
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let auth_code_hash = oauth_server::hash_token(&auth_code);
    let expires_at = (chrono::Utc::now() + chrono::Duration::minutes(5)).to_rfc3339();

    McpClientRegistration::create(
        conn,
        tenant.id,
        setup_code.mcp_identity_id,
        &client_id,
        &auth_code_hash,
        None,
        Some(&expires_at),
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut response = serde_json::json!({
        "code": auth_code,
    });

    if let Some(ref s) = submission.state {
        response["state"] = serde_json::Value::String(s.clone());
    }

    if let Some(ref redirect_uri) = submission.redirect_uri {
        response["redirect_uri"] = serde_json::Value::String(format!(
            "{}?code={}&state={}",
            redirect_uri,
            auth_code,
            submission.state.as_deref().unwrap_or("")
        ));
    }

    Ok(Json(response))
}

// ── T087: Token endpoint (code exchange + refresh) ────────────────────

#[derive(Debug, Deserialize)]
pub struct TokenRequest {
    pub grant_type: String,
    pub code: Option<String>,
    pub client_id: Option<String>,
    pub refresh_token: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
}

/// POST /mcp/token
///
/// Exchange an authorization code for access + refresh tokens,
/// or refresh an existing token.
pub async fn token(
    State(state): State<Arc<crate::server::AppState>>,
    Json(req): Json<TokenRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    match req.grant_type.as_str() {
        "authorization_code" => exchange_code(state, req).await,
        "refresh_token" => refresh_token(state, req).await,
        _ => Err(StatusCode::BAD_REQUEST),
    }
}

async fn exchange_code(
    state: Arc<crate::server::AppState>,
    req: TokenRequest,
) -> Result<Json<TokenResponse>, StatusCode> {
    let code = req.code.as_deref().ok_or(StatusCode::BAD_REQUEST)?;
    let code_hash = oauth_server::hash_token(code);

    // Find registration by the code hash (stored as access_token_hash during authorize)
    let tenants = {
        let conn = state.platform_store.conn();
        crate::store::models::Tenant::list(&conn).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    };

    for tenant in &tenants {
        let tenant_store = state
            .open_tenant_store(&tenant.id)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let conn = tenant_store.conn();

        if let Some(reg) =
            McpClientRegistration::get_by_access_token_hash(conn, &code_hash)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        {
            // Check expiry
            if let Some(ref expires_at) = reg.token_expires_at {
                let now = chrono::Utc::now().to_rfc3339();
                if now > *expires_at {
                    return Err(StatusCode::UNAUTHORIZED);
                }
            }

            // Generate real tokens
            let access_token = oauth_server::generate_token();
            let refresh_token = oauth_server::generate_token();
            let access_hash = oauth_server::hash_token(&access_token);
            let refresh_hash = oauth_server::hash_token(&refresh_token);
            let expires_in: i64 = 3600; // 1 hour
            let expires_at =
                (chrono::Utc::now() + chrono::Duration::seconds(expires_in)).to_rfc3339();

            McpClientRegistration::update_tokens(
                conn,
                reg.id,
                &access_hash,
                Some(&refresh_hash),
                Some(&expires_at),
            )
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

            return Ok(Json(TokenResponse {
                access_token,
                token_type: "bearer".to_string(),
                expires_in,
                refresh_token: Some(refresh_token),
            }));
        }
    }

    Err(StatusCode::UNAUTHORIZED)
}

async fn refresh_token(
    state: Arc<crate::server::AppState>,
    req: TokenRequest,
) -> Result<Json<TokenResponse>, StatusCode> {
    let refresh = req
        .refresh_token
        .as_deref()
        .ok_or(StatusCode::BAD_REQUEST)?;
    let refresh_hash = oauth_server::hash_token(refresh);

    let tenants = {
        let conn = state.platform_store.conn();
        crate::store::models::Tenant::list(&conn).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    };

    for tenant in &tenants {
        let tenant_store = state
            .open_tenant_store(&tenant.id)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let conn = tenant_store.conn();

        // Find registration by refresh token hash
        let mut stmt = conn
            .prepare(
                "SELECT id, tenant_id, mcp_identity_id, client_id, access_token_hash,
                 refresh_token_hash, token_expires_at, created_at, revoked_at
                 FROM mcp_client_registrations
                 WHERE refresh_token_hash = ?1 AND revoked_at IS NULL",
            )
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let reg: Option<McpClientRegistration> = stmt
            .query_map(rusqlite::params![refresh_hash], |row| {
                Ok(McpClientRegistration {
                    id: row.get::<_, String>(0)?.parse().unwrap(),
                    tenant_id: row.get::<_, String>(1)?.parse().unwrap(),
                    mcp_identity_id: row.get::<_, String>(2)?.parse().unwrap(),
                    client_id: row.get(3)?,
                    access_token_hash: row.get(4)?,
                    refresh_token_hash: row.get(5)?,
                    token_expires_at: row.get(6)?,
                    created_at: row.get(7)?,
                    revoked_at: row.get(8)?,
                })
            })
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .next()
            .transpose()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        if let Some(reg) = reg {
            let access_token = oauth_server::generate_token();
            let new_refresh = oauth_server::generate_token();
            let access_hash = oauth_server::hash_token(&access_token);
            let new_refresh_hash = oauth_server::hash_token(&new_refresh);
            let expires_in: i64 = 3600;
            let expires_at =
                (chrono::Utc::now() + chrono::Duration::seconds(expires_in)).to_rfc3339();

            McpClientRegistration::update_tokens(
                conn,
                reg.id,
                &access_hash,
                Some(&new_refresh_hash),
                Some(&expires_at),
            )
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

            return Ok(Json(TokenResponse {
                access_token,
                token_type: "bearer".to_string(),
                expires_in,
                refresh_token: Some(new_refresh),
            }));
        }
    }

    Err(StatusCode::UNAUTHORIZED)
}
