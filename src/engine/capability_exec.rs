use crate::store::models::{Capability, CapabilityInvocationLog, CredentialVaultEntry};
use crate::store::Vault;
use std::time::Instant;

/// On 401, attempt to refresh the OAuth token using the stored refresh token (T076).
/// Returns true if the token was refreshed successfully, false otherwise.
async fn try_refresh_token(
    conn: &rusqlite::Connection,
    vault: &Vault,
    credential: &CredentialVaultEntry,
) -> anyhow::Result<bool> {
    // Only applicable to delegated user tokens (OAuth)
    if credential.kind != "delegated_user_token" {
        return Ok(false);
    }

    let _provider = match &credential.provider {
        Some(p) => p.clone(),
        None => return Ok(false),
    };

    // Decrypt the current token to check if it contains refresh info
    let decrypted = vault.decrypt(&credential.encrypted_value)?;
    let token_str = String::from_utf8(decrypted)?;

    // In a full implementation, the stored value would be a JSON object
    // containing both access_token and refresh_token. For now, we check
    // if it looks like it could be refreshed.
    if !token_str.contains("refresh_token") {
        // No refresh token available -- cannot silently refresh
        return Ok(false);
    }

    // Parse the stored token JSON to extract refresh_token
    let token_data: serde_json::Value = match serde_json::from_str(&token_str) {
        Ok(v) => v,
        Err(_) => return Ok(false),
    };

    let refresh_token = match token_data.get("refresh_token").and_then(|v| v.as_str()) {
        Some(rt) => rt.to_string(),
        None => return Ok(false),
    };

    // Attempt to refresh by calling the provider's token endpoint.
    // This is a placeholder -- in production, provider config would be
    // looked up from a registry.
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("https://{}/oauth/token", _provider))
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", &refresh_token),
        ])
        .send()
        .await;

    match resp {
        Ok(r) if r.status().is_success() => {
            let body: serde_json::Value = r.json().await?;
            let new_token_json = serde_json::to_string(&body)?;
            let encrypted = vault.encrypt(new_token_json.as_bytes())?;
            CredentialVaultEntry::update_encrypted_value(conn, credential.id, &encrypted)?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

/// Check if granted scopes cover all required scopes (T077).
/// Returns a list of missing scopes, if any.
fn check_missing_scopes(granted: Option<&str>, required: &[&str]) -> Vec<String> {
    if required.is_empty() {
        return Vec::new();
    }

    let granted_set: std::collections::HashSet<&str> = granted
        .unwrap_or("")
        .split_whitespace()
        .collect();

    required
        .iter()
        .filter(|s| !granted_set.contains(*s))
        .map(|s| s.to_string())
        .collect()
}

/// Execute a capability: resolve credential, build HTTP request, call endpoint, log result.
pub async fn execute_capability(
    conn: &rusqlite::Connection,
    capability: &Capability,
    arguments: &serde_json::Value,
    tenant_id: &uuid::Uuid,
    agent_id: &uuid::Uuid,
    conversation_id: &uuid::Uuid,
    vault: &Vault,
) -> anyhow::Result<serde_json::Value> {
    let start = Instant::now();
    let client = reqwest::Client::new();

    // 1. Build the request
    let mut request = client
        .post(&capability.endpoint_url)
        .header("Content-Type", "application/json")
        .json(arguments);

    // 2. Resolve credential and set auth header if auth_type != "none"
    if capability.auth_type != "none" {
        if let Some(ref cred_name) = capability.credential_ref {
            let entry = CredentialVaultEntry::get_by_name(
                conn,
                *tenant_id,
                cred_name,
                Some(*agent_id),
            )?;

            if let Some(entry) = entry {
                let decrypted = vault.decrypt(&entry.encrypted_value)?;
                let token = String::from_utf8(decrypted)?;

                // Both api_key and oauth use Bearer token
                request = request.header("Authorization", format!("Bearer {}", token));
            } else {
                anyhow::bail!(
                    "Credential '{}' not found for capability '{}'",
                    cred_name,
                    capability.name
                );
            }
        }
    }

    // 3. Execute the request
    let result = request.send().await;
    let elapsed_ms = start.elapsed().as_millis() as i64;

    match result {
        Ok(resp) => {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();

            if status.is_success() {
                // Parse response as JSON, or wrap as string
                let response_value: serde_json::Value =
                    serde_json::from_str(&body_text).unwrap_or_else(|_| {
                        serde_json::json!({ "result": body_text })
                    });

                // Log success
                let _ = CapabilityInvocationLog::create(
                    conn,
                    *tenant_id,
                    *agent_id,
                    capability.id,
                    Some(*conversation_id),
                    true,
                    elapsed_ms,
                    None,
                );

                Ok(response_value)
            } else {
                let error_msg = format!("HTTP {} — {}", status, body_text);

                // Log failure
                let _ = CapabilityInvocationLog::create(
                    conn,
                    *tenant_id,
                    *agent_id,
                    capability.id,
                    Some(*conversation_id),
                    false,
                    elapsed_ms,
                    Some(&error_msg),
                );

                Ok(serde_json::json!({
                    "error": true,
                    "status": status.as_u16(),
                    "message": error_msg,
                }))
            }
        }
        Err(e) => {
            let error_msg = format!("Request failed: {}", e);

            // Log failure
            let _ = CapabilityInvocationLog::create(
                conn,
                *tenant_id,
                *agent_id,
                capability.id,
                Some(*conversation_id),
                false,
                elapsed_ms,
                Some(&error_msg),
            );

            Ok(serde_json::json!({
                "error": true,
                "message": error_msg,
            }))
        }
    }
}
