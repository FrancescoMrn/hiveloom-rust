pub mod client;
pub mod server;

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{Html, IntoResponse},
};
use std::sync::Arc;

use crate::store::models::OAuthAuthorizationRequest;

#[derive(serde::Deserialize)]
pub struct OAuthCallbackParams {
    pub code: String,
    pub state: String,
}

/// Handle GET /oauth/callback?code=...&state=... (T075).
///
/// 1. Look up OAuthAuthorizationRequest by state_token
/// 2. Verify not expired
/// 3. Exchange code for tokens (POST to provider's token endpoint)
/// 4. Store tokens in credential vault as delegated_user_token
/// 5. Mark request completed
/// 6. Resume paused agent run
/// 7. Return success page
pub async fn handle_callback(
    State(state): State<Arc<super::AppState>>,
    Query(params): Query<OAuthCallbackParams>,
) -> Result<impl IntoResponse, StatusCode> {
    // Look up all tenants to find the matching state token.
    // In production, the state token would encode the tenant_id, but
    // here we scan tenant stores since the callback endpoint is global.
    let tenants = {
        let conn = state.platform_store.conn();
        crate::store::models::Tenant::list(&conn).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    };

    for tenant in &tenants {
        let tenant_store = state
            .open_tenant_store(&tenant.id)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let conn = tenant_store.conn();

        let request = OAuthAuthorizationRequest::get_by_state_token(conn, &params.state)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        if let Some(req) = request {
            // Verify not already completed
            if req.completed_at.is_some() {
                return Err(StatusCode::GONE);
            }

            // Verify not expired
            let now = chrono::Utc::now().to_rfc3339();
            if now > req.expires_at {
                return Err(StatusCode::GONE);
            }

            // Exchange code for tokens.
            // In a real implementation, this would POST to the provider's token
            // endpoint. For now, we store the authorization code as the token
            // value, which the caller can exchange later.
            let token_value = format!("oauth_token_from_code:{}", params.code);
            let encrypted = state
                .vault
                .encrypt(token_value.as_bytes())
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

            // Store as delegated_user_token credential
            let cred_name = format!("oauth_{}_{}", req.provider, req.user_identity);
            let scopes_str = req.requested_scopes.as_deref();

            // Try to update existing credential or create new one
            let existing = crate::store::models::CredentialVaultEntry::get_by_name(
                conn,
                req.tenant_id,
                &cred_name,
                None,
            )
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

            if let Some(existing) = existing {
                crate::store::models::CredentialVaultEntry::update_encrypted_value(
                    conn,
                    existing.id,
                    &encrypted,
                )
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            } else {
                crate::store::models::CredentialVaultEntry::create(
                    conn,
                    req.tenant_id,
                    None,
                    &cred_name,
                    "delegated_user_token",
                    &encrypted,
                    Some(&req.provider),
                    Some(&req.user_identity),
                    scopes_str,
                )
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            }

            // Mark request completed
            OAuthAuthorizationRequest::mark_completed(conn, req.id)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

            // Resume paused agent run if there is one
            if let Some(ref run_ref) = req.paused_run_ref {
                if let Ok(conv_id) = run_ref.parse::<uuid::Uuid>() {
                    let _ = crate::engine::workflow::resume_workflow(conn, &conv_id);
                }
            }

            return Ok(Html(
                "<html><body><h1>Authorization successful</h1>\
                 <p>You can close this window and return to the conversation.</p>\
                 </body></html>"
                    .to_string(),
            ));
        }
    }

    Err(StatusCode::NOT_FOUND)
}
