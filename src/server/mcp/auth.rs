use axum::{
    extract::{Form, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Redirect},
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::server::oauth::server as oauth_server;
use crate::store::models::{McpClientRegistration, McpOAuthClient, McpSetupCode};

// ── HTML escaping for XSS prevention ─────────────────────────────────

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

// ── Base URL helper ──────────────────────────────────────────────────

fn external_base_url(headers: &HeaderMap) -> String {
    let proto = header_value(headers, "x-forwarded-proto")
        .or_else(|| header_value(headers, "x-forwarded-protocol"))
        .unwrap_or_else(|| "http".to_string());
    let host = header_value(headers, "x-forwarded-host")
        .or_else(|| header_value(headers, "host"))
        .unwrap_or_else(|| "127.0.0.1:3000".to_string());
    format!("{}://{}", proto, host)
}

fn header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_string())
}

// ── T085: Metadata endpoints ──────────────────────────────────────────

/// GET /.well-known/oauth-authorization-server
///
/// Returns the OAuth Authorization Server metadata per RFC 8414.
pub async fn oauth_metadata(
    State(_state): State<Arc<crate::server::AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let base_url = external_base_url(&headers);
    Json(serde_json::json!({
        "issuer": base_url,
        "authorization_endpoint": format!("{}/oauth/authorize", base_url),
        "token_endpoint": format!("{}/oauth/token", base_url),
        "registration_endpoint": format!("{}/oauth/register", base_url),
        "response_types_supported": ["code"],
        "grant_types_supported": ["authorization_code", "refresh_token"],
        "token_endpoint_auth_methods_supported": ["client_secret_post"],
        "code_challenge_methods_supported": ["S256"],
        "scopes_supported": ["mcp"]
    }))
}

/// GET /mcp/:tenant_slug/:agent_slug/.well-known/oauth-protected-resource
///
/// Returns the protected resource metadata (per-agent).
pub async fn protected_resource_metadata(
    Path((tenant_slug, agent_slug)): Path<(String, String)>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let base_url = external_base_url(&headers);
    Json(serde_json::json!({
        "resource": format!("{}/mcp/{}/{}", base_url, tenant_slug, agent_slug),
        "authorization_servers": [base_url],
        "bearer_methods_supported": ["header"]
    }))
}

// ── Dynamic Client Registration ──────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct RegisterClientRequest {
    pub client_name: Option<String>,
    pub redirect_uris: Vec<String>,
    pub grant_types: Option<Vec<String>>,
    pub response_types: Option<Vec<String>>,
    pub token_endpoint_auth_method: Option<String>,
}

/// POST /oauth/register
///
/// Dynamic client registration. Returns client_id + client_secret.
pub async fn register_client(
    State(state): State<Arc<crate::server::AppState>>,
    Json(body): Json<RegisterClientRequest>,
) -> Result<impl IntoResponse, StatusCode> {
    if body.redirect_uris.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let client_id = format!("mcp_client_{}", uuid::Uuid::new_v4().to_string().replace('-', ""));
    let client_secret = format!("mcp_secret_{}", oauth_server::generate_token());
    let secret_hash = oauth_server::hash_token(&client_secret);

    let grant_types = body
        .grant_types
        .unwrap_or_else(|| vec!["authorization_code".to_string(), "refresh_token".to_string()]);
    let response_types = body.response_types.unwrap_or_else(|| vec!["code".to_string()]);
    let auth_method = body
        .token_endpoint_auth_method
        .as_deref()
        .unwrap_or("client_secret_post");

    let redirect_uris_json = serde_json::to_string(&body.redirect_uris)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let grant_types_json =
        serde_json::to_string(&grant_types).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let conn = state.platform_store.conn();
    McpOAuthClient::create(
        &conn,
        &client_id,
        &secret_hash,
        body.client_name.as_deref(),
        &redirect_uris_json,
        &grant_types_json,
        auth_method,
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({
            "client_id": client_id,
            "client_secret": client_secret,
            "client_name": body.client_name,
            "redirect_uris": body.redirect_uris,
            "grant_types": grant_types,
            "response_types": response_types,
            "token_endpoint_auth_method": auth_method
        })),
    ))
}

// ── Authorization Endpoint ───────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct AuthorizeParams {
    pub response_type: Option<String>,
    pub client_id: Option<String>,
    pub state: Option<String>,
    pub redirect_uri: Option<String>,
    pub code_challenge: Option<String>,
    pub code_challenge_method: Option<String>,
    pub scope: Option<String>,
}

fn render_authorize_html(
    params: &AuthorizeParams,
    error_message: Option<&str>,
) -> Html<String> {
    let client_id = html_escape(params.client_id.as_deref().unwrap_or(""));
    let redirect_uri = html_escape(params.redirect_uri.as_deref().unwrap_or(""));
    let state = html_escape(params.state.as_deref().unwrap_or(""));
    let code_challenge = html_escape(params.code_challenge.as_deref().unwrap_or(""));
    let code_challenge_method = html_escape(params.code_challenge_method.as_deref().unwrap_or(""));

    let error_html = match error_message {
        Some(msg) => format!(r#"<p class="error">{}</p>"#, html_escape(msg)),
        None => String::new(),
    };

    Html(format!(
        r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<title>Hiveloom - Authorize MCP Client</title>
<style>
  body {{ font-family: system-ui, -apple-system, sans-serif; max-width: 420px; margin: 80px auto; padding: 0 20px; color: #333; }}
  h1 {{ font-size: 1.4em; margin-bottom: 0.5em; }}
  p {{ color: #666; line-height: 1.5; }}
  input[type=text] {{ width: 100%; padding: 10px; margin: 8px 0 16px 0; box-sizing: border-box; font-size: 16px; border: 1px solid #ccc; border-radius: 4px; }}
  button {{ padding: 10px 24px; font-size: 16px; cursor: pointer; background: #2563eb; color: white; border: none; border-radius: 4px; }}
  button:hover {{ background: #1d4ed8; }}
  .error {{ color: #dc2626; background: #fef2f2; padding: 10px; border-radius: 4px; border: 1px solid #fecaca; }}
</style>
</head>
<body>
<h1>Authorize MCP Client</h1>
<p>Enter the setup code provided by your administrator.</p>
{error_html}
<form method="POST" action="/oauth/authorize">
  <input type="text" name="setup_code" placeholder="Setup code" required autofocus autocomplete="off">
  <input type="hidden" name="client_id" value="{client_id}">
  <input type="hidden" name="redirect_uri" value="{redirect_uri}">
  <input type="hidden" name="state" value="{state}">
  <input type="hidden" name="code_challenge" value="{code_challenge}">
  <input type="hidden" name="code_challenge_method" value="{code_challenge_method}">
  <button type="submit">Authorize</button>
</form>
</body>
</html>"#
    ))
}

/// GET /oauth/authorize
///
/// Renders the HTML setup code entry form.
pub async fn authorize(Query(params): Query<AuthorizeParams>) -> impl IntoResponse {
    if params.response_type.as_deref() != Some("code") {
        return (
            StatusCode::BAD_REQUEST,
            Html("<h1>Error</h1><p>Unsupported response_type. Expected: code</p>".to_string()),
        )
            .into_response();
    }

    render_authorize_html(&params, None).into_response()
}

#[derive(Debug, Deserialize)]
pub struct AuthorizeFormSubmission {
    pub setup_code: String,
    pub client_id: String,
    pub redirect_uri: String,
    pub state: String,
    pub code_challenge: Option<String>,
    pub code_challenge_method: Option<String>,
}

/// POST /oauth/authorize
///
/// Validates the setup code, creates a client registration, and redirects
/// back to the client with an authorization code.
pub async fn authorize_submit(
    State(state): State<Arc<crate::server::AppState>>,
    Form(submission): Form<AuthorizeFormSubmission>,
) -> impl IntoResponse {
    // Build params for re-rendering the form on error
    let form_params = AuthorizeParams {
        response_type: Some("code".to_string()),
        client_id: Some(submission.client_id.clone()),
        state: Some(submission.state.clone()),
        redirect_uri: Some(submission.redirect_uri.clone()),
        code_challenge: submission.code_challenge.clone(),
        code_challenge_method: submission.code_challenge_method.clone(),
        scope: None,
    };

    // Verify the client_id exists in the platform store
    {
        let conn = state.platform_store.conn();
        match McpOAuthClient::get_by_client_id(&conn, &submission.client_id) {
            Ok(Some(client)) => {
                // Verify redirect_uri matches one of the registered URIs
                let uris: Vec<String> = serde_json::from_str(&client.redirect_uris)
                    .unwrap_or_default();
                if !uris.contains(&submission.redirect_uri) {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(serde_json::json!({
                            "error": "invalid_request",
                            "error_description": "Unknown client or redirect URI mismatch"
                        })),
                    )
                        .into_response();
                }
            }
            Ok(None) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "error": "invalid_request",
                        "error_description": "Unknown client or redirect URI mismatch"
                    })),
                )
                    .into_response();
            }
            Err(_) => {
                return render_authorize_html(&form_params, Some("Internal error"))
                    .into_response();
            }
        }
    }

    // Hash the setup code and search all tenants for a match
    let code_hash = oauth_server::hash_token(&submission.setup_code);

    let tenants = {
        let conn = state.platform_store.conn();
        match crate::store::models::Tenant::list(&conn) {
            Ok(t) => t,
            Err(_) => {
                return render_authorize_html(&form_params, Some("Internal error")).into_response()
            }
        }
    };

    for tenant in &tenants {
        let tenant_store = match state.open_tenant_store(&tenant.id) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let conn = tenant_store.conn();

        let setup_code = match McpSetupCode::get_valid_by_hash(conn, &code_hash) {
            Ok(Some(sc)) => sc,
            Ok(None) => continue,
            Err(_) => continue,
        };

        // Mark the setup code as used
        if McpSetupCode::mark_used(conn, setup_code.id).is_err() {
            return render_authorize_html(&form_params, Some("Internal error")).into_response();
        }

        // Remove any existing registration for this client_id (re-authorization).
        // The client_id column has a UNIQUE constraint, so we must delete the
        // old registration before creating a new one with the fresh auth code.
        if let Ok(Some(existing)) = McpClientRegistration::get_by_client_id(conn, &submission.client_id) {
            let _ = McpClientRegistration::delete(conn, existing.id);
        }

        // Generate an authorization code
        let auth_code = oauth_server::generate_token();
        let auth_code_hash = oauth_server::hash_token(&auth_code);
        let expires_at = (chrono::Utc::now() + chrono::Duration::minutes(5)).to_rfc3339();

        if McpClientRegistration::create(
            conn,
            tenant.id,
            setup_code.mcp_identity_id,
            &submission.client_id,
            &auth_code_hash,
            None,
            Some(&expires_at),
            submission.code_challenge.as_deref(),
            submission.code_challenge_method.as_deref(),
            Some(&submission.redirect_uri),
        )
        .is_err()
        {
            return render_authorize_html(&form_params, Some("Internal error")).into_response();
        }

        // Redirect to client
        let redirect_url = format!(
            "{}?code={}&state={}",
            submission.redirect_uri, auth_code, submission.state
        );
        return Redirect::to(&redirect_url).into_response();
    }

    // No matching setup code found in any tenant
    render_authorize_html(&form_params, Some("Invalid or expired setup code")).into_response()
}

// ── Token Endpoint ───────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct TokenRequest {
    pub grant_type: String,
    pub code: Option<String>,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub redirect_uri: Option<String>,
    pub code_verifier: Option<String>,
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

#[derive(Debug, Serialize)]
pub struct TokenErrorResponse {
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_description: Option<String>,
}

/// POST /oauth/token
///
/// Exchange an authorization code for access + refresh tokens,
/// or refresh an existing token.
pub async fn token(
    State(state): State<Arc<crate::server::AppState>>,
    Form(req): Form<TokenRequest>,
) -> impl IntoResponse {
    match req.grant_type.as_str() {
        "authorization_code" => exchange_code(state, req).await.into_response(),
        "refresh_token" => refresh_token(state, req).await.into_response(),
        _ => (
            StatusCode::BAD_REQUEST,
            Json(TokenErrorResponse {
                error: "unsupported_grant_type".to_string(),
                error_description: None,
            }),
        )
            .into_response(),
    }
}

async fn exchange_code(
    state: Arc<crate::server::AppState>,
    req: TokenRequest,
) -> Result<Json<TokenResponse>, (StatusCode, Json<TokenErrorResponse>)> {
    let code = req.code.as_deref().ok_or((
        StatusCode::BAD_REQUEST,
        Json(TokenErrorResponse {
            error: "invalid_request".to_string(),
            error_description: Some("Missing code".to_string()),
        }),
    ))?;
    let client_id = req.client_id.as_deref().ok_or((
        StatusCode::BAD_REQUEST,
        Json(TokenErrorResponse {
            error: "invalid_request".to_string(),
            error_description: Some("Missing client_id".to_string()),
        }),
    ))?;
    let client_secret = req.client_secret.as_deref().ok_or((
        StatusCode::BAD_REQUEST,
        Json(TokenErrorResponse {
            error: "invalid_request".to_string(),
            error_description: Some("Missing client_secret".to_string()),
        }),
    ))?;

    // Verify client credentials in platform store
    {
        let conn = state.platform_store.conn();
        let oauth_client = McpOAuthClient::get_by_client_id(&conn, client_id)
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(TokenErrorResponse {
                        error: "server_error".to_string(),
                        error_description: None,
                    }),
                )
            })?
            .ok_or((
                StatusCode::UNAUTHORIZED,
                Json(TokenErrorResponse {
                    error: "invalid_client".to_string(),
                    error_description: None,
                }),
            ))?;

        let secret_hash = oauth_server::hash_token(client_secret);
        if secret_hash != oauth_client.client_secret_hash {
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(TokenErrorResponse {
                    error: "invalid_client".to_string(),
                    error_description: None,
                }),
            ));
        }
    }

    let code_hash = oauth_server::hash_token(code);

    let tenants = {
        let conn = state.platform_store.conn();
        crate::store::models::Tenant::list(&conn).map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(TokenErrorResponse {
                    error: "server_error".to_string(),
                    error_description: None,
                }),
            )
        })?
    };

    for tenant in &tenants {
        let tenant_store = match state.open_tenant_store(&tenant.id) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let conn = tenant_store.conn();

        if let Some(reg) = McpClientRegistration::get_by_access_token_hash(conn, &code_hash)
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(TokenErrorResponse {
                        error: "server_error".to_string(),
                        error_description: None,
                    }),
                )
            })?
        {
            // Check expiry
            if let Some(ref expires_at) = reg.token_expires_at {
                let now = chrono::Utc::now().to_rfc3339();
                if now > *expires_at {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(TokenErrorResponse {
                            error: "invalid_grant".to_string(),
                            error_description: Some("Authorization code expired".to_string()),
                        }),
                    ));
                }
            }

            // Verify redirect_uri matches
            if let Some(ref stored_uri) = reg.redirect_uri {
                if req.redirect_uri.as_deref() != Some(stored_uri.as_str()) {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(TokenErrorResponse {
                            error: "invalid_grant".to_string(),
                            error_description: Some("redirect_uri mismatch".to_string()),
                        }),
                    ));
                }
            }

            // Verify PKCE
            if let Some(ref stored_challenge) = reg.code_challenge {
                let verifier = req.code_verifier.as_deref().ok_or((
                    StatusCode::BAD_REQUEST,
                    Json(TokenErrorResponse {
                        error: "invalid_grant".to_string(),
                        error_description: Some("Missing code_verifier".to_string()),
                    }),
                ))?;
                if !oauth_server::verify_pkce(verifier, stored_challenge) {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(TokenErrorResponse {
                            error: "invalid_grant".to_string(),
                            error_description: Some("PKCE verification failed".to_string()),
                        }),
                    ));
                }
            }

            // Generate real tokens
            let access_token = oauth_server::generate_token();
            let refresh_token = oauth_server::generate_token();
            let access_hash = oauth_server::hash_token(&access_token);
            let refresh_hash = oauth_server::hash_token(&refresh_token);
            let expires_in: i64 = 3600;
            let expires_at =
                (chrono::Utc::now() + chrono::Duration::seconds(expires_in)).to_rfc3339();

            McpClientRegistration::update_tokens(
                conn,
                reg.id,
                &access_hash,
                Some(&refresh_hash),
                Some(&expires_at),
            )
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(TokenErrorResponse {
                        error: "server_error".to_string(),
                        error_description: None,
                    }),
                )
            })?;

            return Ok(Json(TokenResponse {
                access_token,
                token_type: "Bearer".to_string(),
                expires_in,
                refresh_token: Some(refresh_token),
            }));
        }
    }

    Err((
        StatusCode::BAD_REQUEST,
        Json(TokenErrorResponse {
            error: "invalid_grant".to_string(),
            error_description: None,
        }),
    ))
}

async fn refresh_token(
    state: Arc<crate::server::AppState>,
    req: TokenRequest,
) -> Result<Json<TokenResponse>, (StatusCode, Json<TokenErrorResponse>)> {
    let refresh = req.refresh_token.as_deref().ok_or((
        StatusCode::BAD_REQUEST,
        Json(TokenErrorResponse {
            error: "invalid_request".to_string(),
            error_description: Some("Missing refresh_token".to_string()),
        }),
    ))?;
    let client_id = req.client_id.as_deref().ok_or((
        StatusCode::BAD_REQUEST,
        Json(TokenErrorResponse {
            error: "invalid_request".to_string(),
            error_description: Some("Missing client_id".to_string()),
        }),
    ))?;
    let client_secret = req.client_secret.as_deref().ok_or((
        StatusCode::BAD_REQUEST,
        Json(TokenErrorResponse {
            error: "invalid_request".to_string(),
            error_description: Some("Missing client_secret".to_string()),
        }),
    ))?;

    // Verify client credentials
    {
        let conn = state.platform_store.conn();
        let oauth_client = McpOAuthClient::get_by_client_id(&conn, client_id)
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(TokenErrorResponse {
                        error: "server_error".to_string(),
                        error_description: None,
                    }),
                )
            })?
            .ok_or((
                StatusCode::UNAUTHORIZED,
                Json(TokenErrorResponse {
                    error: "invalid_client".to_string(),
                    error_description: None,
                }),
            ))?;

        let secret_hash = oauth_server::hash_token(client_secret);
        if secret_hash != oauth_client.client_secret_hash {
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(TokenErrorResponse {
                    error: "invalid_client".to_string(),
                    error_description: None,
                }),
            ));
        }
    }

    let refresh_hash = oauth_server::hash_token(refresh);

    let tenants = {
        let conn = state.platform_store.conn();
        crate::store::models::Tenant::list(&conn).map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(TokenErrorResponse {
                    error: "server_error".to_string(),
                    error_description: None,
                }),
            )
        })?
    };

    for tenant in &tenants {
        let tenant_store = match state.open_tenant_store(&tenant.id) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let conn = tenant_store.conn();

        // Find registration by refresh token hash
        let reg = McpClientRegistration::get_by_refresh_token_hash(conn, &refresh_hash);
        let reg = match reg {
            Ok(Some(r)) => r,
            Ok(None) => continue,
            Err(_) => continue,
        };

        if reg.revoked_at.is_some() {
            continue;
        }

        // Check that the MCP identity is still active
        if let Ok(Some(identity)) =
            crate::store::models::McpIdentity::get(conn, reg.mcp_identity_id)
        {
            if identity.status != "active" {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(TokenErrorResponse {
                        error: "invalid_grant".to_string(),
                        error_description: Some("Identity revoked".to_string()),
                    }),
                ));
            }
        }

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
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(TokenErrorResponse {
                    error: "server_error".to_string(),
                    error_description: None,
                }),
            )
        })?;

        return Ok(Json(TokenResponse {
            access_token,
            token_type: "Bearer".to_string(),
            expires_in,
            refresh_token: Some(new_refresh),
        }));
    }

    Err((
        StatusCode::BAD_REQUEST,
        Json(TokenErrorResponse {
            error: "invalid_grant".to_string(),
            error_description: None,
        }),
    ))
}
