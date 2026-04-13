use crate::store::models::{Agent, Capability, Conversation, EventSubscription};
use crate::store::TenantStore;

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
    let store = TenantStore::open(std::path::Path::new(data_dir), tenant_id)?;
    let conn = store.conn();

    // Find matching subscriptions
    let subscriptions = EventSubscription::list_by_event_type(conn, *tenant_id, event_type)?;

    if subscriptions.is_empty() {
        tracing::debug!(
            tenant_id = %tenant_id,
            event_type = %event_type,
            "No active subscriptions for event type"
        );
        return Ok(());
    }

    // Hash the provided auth token for comparison
    use sha2::{Digest, Sha256};
    let token_hash = hex::encode(Sha256::digest(auth_token.as_bytes()));

    let payload_str = serde_json::to_string_pretty(payload)?;
    let source = payload.get("source").and_then(|v| v.as_str());

    for sub in &subscriptions {
        // Verify auth token
        if sub.auth_token_hash != token_hash {
            tracing::debug!(
                subscription_id = %sub.id,
                "Auth token mismatch, skipping subscription"
            );
            continue;
        }

        // Apply source_filter if present
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

        // Load the agent
        let agent = match Agent::get_current(conn, *tenant_id, sub.agent_id)? {
            Some(a) => a,
            None => {
                tracing::warn!(
                    agent_id = %sub.agent_id,
                    subscription_id = %sub.id,
                    "Agent not found for subscription"
                );
                continue;
            }
        };

        if agent.status != "active" {
            tracing::debug!(
                agent_id = %sub.agent_id,
                "Agent is not active, skipping"
            );
            continue;
        }

        // Load capabilities
        let capabilities = Capability::list_by_agent(conn, *tenant_id, sub.agent_id)?;

        // Create a synthetic internal conversation for the event-driven run
        let conversation = Conversation::create(
            conn,
            *tenant_id,
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

        let invocation = crate::engine::AgentInvocation {
            agent,
            capabilities,
            conversation_id: conversation.id,
            tenant_id: *tenant_id,
            user_identity: "system".to_string(),
        };

        tracing::info!(
            subscription_id = %sub.id,
            agent_id = %sub.agent_id,
            conversation_id = %conversation.id,
            event_type = %event_type,
            initial_message = %initial_message,
            "Event routed to agent (invocation conversation = {})",
            invocation.conversation_id,
        );

        // Conclude the conversation after dispatch
        Conversation::update_status(conn, conversation.id, "concluded")?;
    }

    Ok(())
}
