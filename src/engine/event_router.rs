use crate::store::models::{
    Agent, Capability, Conversation, CredentialVaultEntry, EventSubscription,
};
use crate::store::{TenantStore, Vault};

/// Route an inbound event to matching subscriptions and invoke agents.
///
/// 1. Open tenant store
/// 2. Find matching EventSubscriptions for event_type
/// 3. Verify auth_token against subscription's auth_token_hash
/// 4. Apply source_filter if present
/// 5. For each match, invoke agent with payload as initial context
pub async fn route_event(
    data_dir: &str,
    tenant_id: &uuid::Uuid,
    event_type: &str,
    payload: &serde_json::Value,
    auth_token: &str,
) -> anyhow::Result<()> {
    let data_dir = data_dir.to_string();
    let tenant_id = *tenant_id;
    let event_type = event_type.to_string();
    let payload = payload.clone();
    let auth_token = auth_token.to_string();

    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let data_path = std::path::Path::new(&data_dir);
        let store = TenantStore::open(data_path, &tenant_id)?;
        let conn = store.conn();
        let vault = Vault::open(data_path)?;

        let subscriptions = EventSubscription::list_by_event_type(conn, tenant_id, &event_type)?;
        if subscriptions.is_empty() {
            tracing::debug!(
                tenant_id = %tenant_id,
                event_type = %event_type,
                "No active subscriptions for event type"
            );
            return Ok(());
        }

        use sha2::{Digest, Sha256};
        let token_hash = hex::encode(Sha256::digest(auth_token.as_bytes()));
        let payload_str = serde_json::to_string_pretty(&payload)?;
        let source = payload.get("source").and_then(|v| v.as_str());

        for sub in &subscriptions {
            if sub.auth_token_hash != token_hash {
                tracing::debug!(
                    subscription_id = %sub.id,
                    "Auth token mismatch, skipping subscription"
                );
                continue;
            }

            if let Some(ref filter) = sub.source_filter {
                if !filter.is_empty() {
                    match source {
                        Some(src) if src == filter => {}
                        _ => {
                            tracing::debug!(
                                subscription_id = %sub.id,
                                source_filter = %filter,
                                "Source filter mismatch, skipping subscription"
                            );
                            continue;
                        }
                    }
                }
            }

            if let Err(e) =
                run_subscription(conn, &vault, tenant_id, sub, &event_type, &payload_str)
            {
                tracing::error!(
                    subscription_id = %sub.id,
                    agent_id = %sub.agent_id,
                    error = %e,
                    "Event subscription failed"
                );
            }
        }

        Ok(())
    })
    .await?
}

fn run_subscription(
    conn: &rusqlite::Connection,
    vault: &Vault,
    tenant_id: uuid::Uuid,
    sub: &EventSubscription,
    event_type: &str,
    payload_str: &str,
) -> anyhow::Result<()> {
    let agent = match Agent::get_current(conn, tenant_id, sub.agent_id)? {
        Some(a) => a,
        None => {
            tracing::warn!(
                agent_id = %sub.agent_id,
                subscription_id = %sub.id,
                "Agent not found for subscription"
            );
            return Ok(());
        }
    };

    if agent.status != "active" {
        tracing::debug!(
            agent_id = %sub.agent_id,
            "Agent is not active, skipping"
        );
        return Ok(());
    }

    let capabilities = Capability::list_by_agent(conn, tenant_id, sub.agent_id)?;
    let conversation = Conversation::create(
        conn,
        tenant_id,
        sub.agent_id,
        "internal",
        &format!("event:{}:{}", event_type, sub.id),
        "system",
        None,
    )?;

    let initial_message = format!(
        "You are responding to an inbound event.\nEvent type: {}\nPayload:\n{}",
        event_type, payload_str
    );

    let credential_name = if agent.model_id.starts_with("claude-") {
        "anthropic"
    } else {
        "openai"
    };
    let credential = CredentialVaultEntry::get_by_name(conn, tenant_id, credential_name, None)?
        .ok_or_else(|| anyhow::anyhow!("No LLM credential '{}' found", credential_name))?;
    let api_key = String::from_utf8(vault.decrypt(&credential.encrypted_value)?)?;
    let provider = crate::llm::resolve_provider(&agent.model_id, &api_key)?;

    let invocation = crate::engine::AgentInvocation {
        agent,
        capabilities,
        conversation_id: conversation.id,
        tenant_id,
        user_identity: "system".to_string(),
    };

    let rt = tokio::runtime::Handle::current();
    let result = rt.block_on(crate::engine::agent_loop::run_agent_loop_with_vault(
        &invocation,
        provider.as_ref(),
        conn,
        &initial_message,
        vault,
    ))?;

    tracing::info!(
        subscription_id = %sub.id,
        agent_id = %sub.agent_id,
        conversation_id = %conversation.id,
        event_type = %event_type,
        tool_calls = ?result.tool_calls_made,
        "Event routed through agent loop"
    );

    Conversation::update_status(conn, conversation.id, "concluded")?;
    Ok(())
}
