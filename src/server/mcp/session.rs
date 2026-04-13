use crate::server::oauth::server as oauth_server;
use crate::store::models::{McpClientRegistration, McpIdentity};

/// Resolved MCP session after token validation.
#[derive(Debug, Clone)]
pub struct McpSession {
    pub registration: McpClientRegistration,
    pub identity: McpIdentity,
    pub user_identity: String,
}

/// Validate an access token and resolve the full MCP session (T083).
///
/// Flow: hash token -> find McpClientRegistration -> find McpIdentity
///       -> resolve mapped person -> set user_identity
pub fn validate_session(
    conn: &rusqlite::Connection,
    access_token: &str,
) -> anyhow::Result<Option<McpSession>> {
    let token_hash = oauth_server::hash_token(access_token);

    // Look up registration by access token hash
    let registration = match McpClientRegistration::get_by_access_token_hash(conn, &token_hash)? {
        Some(r) => r,
        None => return Ok(None),
    };

    // Check if token has expired
    if let Some(ref expires_at) = registration.token_expires_at {
        let now = chrono::Utc::now().to_rfc3339();
        if now > *expires_at {
            return Ok(None);
        }
    }

    // Check if registration is revoked
    if registration.revoked_at.is_some() {
        return Ok(None);
    }

    // Resolve McpIdentity
    let identity = match McpIdentity::get(conn, registration.mcp_identity_id)? {
        Some(id) => id,
        None => return Ok(None),
    };

    // Identity must be active
    if identity.status != "active" {
        return Ok(None);
    }

    // Determine user_identity: prefer mapped_person_id, fall back to identity name
    let user_identity = identity
        .mapped_person_id
        .clone()
        .unwrap_or_else(|| format!("mcp:{}", identity.name));

    Ok(Some(McpSession {
        registration,
        identity,
        user_identity,
    }))
}
