use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Redirect},
};
use serde::Deserialize;
use std::sync::Arc;

use crate::store::models::{CredentialVaultEntry, OAuthAuthorizationRequest, Tenant};

fn html_page(title: &str, message: &str) -> Html<String> {
    Html(format!(
        "<html><body><h1>{}</h1><p>{}</p></body></html>",
        title, message
    ))
}

fn resolve_tenant_by_slug(
    state: &crate::server::AppState,
    tenant_slug: &str,
) -> Result<Tenant, StatusCode> {
    let conn = state.platform_store.conn();
    match Tenant::get_by_slug(&conn, tenant_slug).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)? {
        Some(tenant) => Ok(tenant),
        None => Err(StatusCode::NOT_FOUND),
    }
}

fn find_request_by_state(
    state: &crate::server::AppState,
    state_token: &str,
) -> Result<Option<(Tenant, OAuthAuthorizationRequest)>, StatusCode> {
    let tenants = {
        let conn = state.platform_store.conn();
        Tenant::list(&conn).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    };

    for tenant in tenants {
        let tenant_store = state
            .open_tenant_store(&tenant.id)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let conn = tenant_store.conn();
        let request = OAuthAuthorizationRequest::get_by_state_token(conn, state_token)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        if let Some(request) = request {
            return Ok(Some((tenant, request)));
        }
    }

    Ok(None)
}

fn redirect_uri(headers: &HeaderMap) -> String {
    format!(
        "{}/slack/oauth/callback",
        crate::server::external_base_url(headers)
    )
}

#[derive(Default, Deserialize)]
pub struct InstallQuery {
    pub tenant: Option<String>,
}

/// GET /slack/install?tenant=default
pub async fn start_install(
    State(state): State<Arc<crate::server::AppState>>,
    Query(query): Query<InstallQuery>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, (StatusCode, Html<String>)> {
    let slack_config = state.slack_config.as_ref().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            html_page("Slack Not Configured", "Set SLACK_SIGNING_SECRET first."),
        )
    })?;

    let client_id = slack_config.client_id.as_deref().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            html_page(
                "Slack Install Not Ready",
                "Set SLACK_CLIENT_ID and SLACK_CLIENT_SECRET to enable app installation.",
            ),
        )
    })?;

    if slack_config.client_secret.is_none() {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            html_page(
                "Slack Install Not Ready",
                "Set SLACK_CLIENT_ID and SLACK_CLIENT_SECRET to enable app installation.",
            ),
        ));
    }

    let tenant_slug = query.tenant.unwrap_or_else(|| "default".to_string());
    let tenant = resolve_tenant_by_slug(&state, &tenant_slug).map_err(|status| {
        (
            status,
            html_page("Tenant Not Found", "The requested tenant does not exist."),
        )
    })?;

    let tenant_store = state.open_tenant_store(&tenant.id).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            html_page("Slack Install Error", "Could not open the tenant store."),
        )
    })?;
    let conn = tenant_store.conn();
    let _ = OAuthAuthorizationRequest::cleanup_expired(conn);

    let state_token = crate::server::oauth::server::generate_token();
    let expires_at = (chrono::Utc::now() + chrono::Duration::minutes(10)).to_rfc3339();

    OAuthAuthorizationRequest::create(
        conn,
        tenant.id,
        "slack-install",
        "slack",
        &state_token,
        Some(super::DEFAULT_INSTALL_SCOPES),
        None,
        Some("slack"),
        &expires_at,
    )
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            html_page(
                "Slack Install Error",
                "Could not create the install request.",
            ),
        )
    })?;

    let redirect_uri = redirect_uri(&headers);
    let url = url::Url::parse_with_params(
        "https://slack.com/oauth/v2/authorize",
        &[
            ("client_id", client_id),
            ("scope", super::DEFAULT_INSTALL_SCOPES),
            ("redirect_uri", redirect_uri.as_str()),
            ("state", state_token.as_str()),
        ],
    )
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            html_page(
                "Slack Install Error",
                "Could not construct the Slack install URL.",
            ),
        )
    })?;

    Ok(Redirect::temporary(url.as_str()))
}

#[derive(Deserialize)]
pub struct SlackInstallCallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
}

#[derive(Deserialize)]
struct SlackOauthAccessResponse {
    ok: bool,
    access_token: Option<String>,
    scope: Option<String>,
    app_id: Option<String>,
    error: Option<String>,
    team: Option<SlackTeamInfo>,
}

#[derive(Deserialize)]
struct SlackTeamInfo {
    id: Option<String>,
    name: Option<String>,
}

/// GET /slack/oauth/callback?code=...&state=...
pub async fn handle_install_callback(
    State(state): State<Arc<crate::server::AppState>>,
    Query(query): Query<SlackInstallCallbackQuery>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, StatusCode> {
    if let Some(error) = query.error {
        return Ok((
            StatusCode::BAD_REQUEST,
            html_page(
                "Slack Install Cancelled",
                &format!("Slack returned: {}", error),
            ),
        ));
    }

    let code = query.code.ok_or(StatusCode::BAD_REQUEST)?;
    let state_token = query.state.ok_or(StatusCode::BAD_REQUEST)?;

    let slack_config = state
        .slack_config
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let client_id = slack_config
        .client_id
        .as_deref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let client_secret = slack_config
        .client_secret
        .as_deref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;

    let (tenant, request) =
        find_request_by_state(&state, &state_token)?.ok_or(StatusCode::NOT_FOUND)?;

    if request.completed_at.is_some() {
        return Err(StatusCode::GONE);
    }
    if chrono::Utc::now().to_rfc3339() > request.expires_at {
        return Err(StatusCode::GONE);
    }

    let redirect_uri = redirect_uri(&headers);
    let client = reqwest::Client::new();
    let response = client
        .post("https://slack.com/api/oauth.v2.access")
        .form(&[
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("code", code.as_str()),
            ("redirect_uri", redirect_uri.as_str()),
        ])
        .send()
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "Slack OAuth token exchange request failed");
            StatusCode::BAD_GATEWAY
        })?;

    let oauth_response: SlackOauthAccessResponse = response.json().await.map_err(|e| {
        tracing::error!(error = %e, "Slack OAuth token exchange response parse failed");
        StatusCode::BAD_GATEWAY
    })?;

    if !oauth_response.ok {
        let message = oauth_response
            .error
            .unwrap_or_else(|| "unknown_error".to_string());
        tracing::warn!(error = %message, "Slack OAuth token exchange was rejected");
        let guidance = if message == "invalid_code" {
            "Slack said the authorization code is no longer valid. Start the install flow again from /slack/install and approve the app once more."
        } else {
            "Slack rejected the install request. Check the app configuration and try the install flow again."
        };
        return Ok((
            StatusCode::OK,
            html_page(
                "Slack Install Failed",
                &format!("Slack returned: {}. {}", message, guidance),
            ),
        ));
    }

    let access_token = oauth_response
        .access_token
        .clone()
        .ok_or(StatusCode::BAD_GATEWAY)?;

    let team_identity = oauth_response
        .team
        .as_ref()
        .and_then(|team| team.id.as_deref())
        .unwrap_or("workspace");
    let scope_text = oauth_response.scope.as_deref();

    let encrypted = state
        .vault
        .encrypt(access_token.as_bytes())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let tenant_store = state
        .open_tenant_store(&tenant.id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let conn = tenant_store.conn();

    let existing = CredentialVaultEntry::get_by_name(
        conn,
        tenant.id,
        super::ACCESS_TOKEN_CREDENTIAL_NAME,
        None,
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if let Some(existing) = existing {
        CredentialVaultEntry::update_encrypted_value(conn, existing.id, &encrypted)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    } else {
        CredentialVaultEntry::create(
            conn,
            tenant.id,
            None,
            super::ACCESS_TOKEN_CREDENTIAL_NAME,
            "oauth2",
            &encrypted,
            Some("slack"),
            Some(team_identity),
            scope_text,
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }

    OAuthAuthorizationRequest::mark_completed(conn, request.id)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let team_name = oauth_response
        .team
        .as_ref()
        .and_then(|team| team.name.as_deref())
        .unwrap_or("your Slack workspace");
    let token_kind = crate::server::slack_token_kind(&access_token);
    let token_kind = match token_kind {
        crate::server::SlackTokenKind::Bot => "bot",
        crate::server::SlackTokenKind::User => "user",
        crate::server::SlackTokenKind::App => "app",
        crate::server::SlackTokenKind::Unknown => "unknown",
    };
    let app_id_note = oauth_response
        .app_id
        .or_else(|| slack_config.app_id.clone())
        .unwrap_or_else(|| "unknown".to_string());

    Ok((
        StatusCode::OK,
        html_page(
            "Slack Installed",
            &format!(
                "Hiveloom Demo is now installed in {}. Stored a {} access token for tenant '{}'. App ID: {}. Invite the app to a channel, then bind that channel to an agent.",
                team_name, token_kind, tenant.slug, app_id_note
            ),
        ),
    ))
}
