use crate::store::models::{Capability, CapabilityInvocationLog, CredentialVaultEntry};
use crate::store::Vault;
use std::time::Instant;

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
