use crate::store::models::OAuthAuthorizationRequest;

/// Initiate OAuth authorization flow for a capability (T074).
///
/// Generates a state token, persists an OAuthAuthorizationRequest with a 10-minute
/// expiry, and returns the authorization URL to redirect the user to.
#[allow(clippy::too_many_arguments)]
pub fn create_authorization_url(
    conn: &rusqlite::Connection,
    tenant_id: &uuid::Uuid,
    user_identity: &str,
    provider: &str,
    scopes: &[String],
    callback_url: &str,
    paused_run_ref: &str,
    surface_type: &str,
) -> anyhow::Result<String> {
    // Generate a random state token
    let state_token = uuid::Uuid::new_v4().to_string();

    // Expires in 10 minutes
    let expires_at = (chrono::Utc::now() + chrono::Duration::minutes(10)).to_rfc3339();

    let scopes_str = scopes.join(" ");

    // Persist the authorization request
    OAuthAuthorizationRequest::create(
        conn,
        *tenant_id,
        user_identity,
        provider,
        &state_token,
        Some(&scopes_str),
        Some(paused_run_ref),
        Some(surface_type),
        &expires_at,
    )?;

    // Build the authorization URL.
    // In a real deployment, provider-specific auth endpoints and client_id
    // would be looked up from configuration. Here we construct a generic
    // OAuth 2.0 authorization URL pattern.
    let auth_url = format!(
        "https://{provider}/oauth/authorize\
         ?client_id={client_id}\
         &redirect_uri={redirect_uri}\
         &scope={scope}\
         &state={state}\
         &response_type=code",
        provider = provider,
        client_id = "CONFIGURED_CLIENT_ID",
        redirect_uri = urlencoding::encode(callback_url),
        scope = urlencoding::encode(&scopes_str),
        state = urlencoding::encode(&state_token),
    );

    Ok(auth_url)
}

/// Simple URL-encoding helper (percent-encode non-ASCII, spaces, special chars).
mod urlencoding {
    pub fn encode(input: &str) -> String {
        let mut result = String::with_capacity(input.len() * 3);
        for byte in input.bytes() {
            match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    result.push(byte as char);
                }
                _ => {
                    result.push_str(&format!("%{:02X}", byte));
                }
            }
        }
        result
    }
}
