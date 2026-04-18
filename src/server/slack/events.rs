use std::sync::Arc;

use crate::engine::agent_loop::{run_agent_loop, AgentInvocation};
use crate::store::models::{
    Agent, Capability, ChatSurfaceBinding, Conversation, CredentialVaultEntry, Tenant,
};

use super::SlackSurface;
use crate::engine::chat_surface::ChatSurface;

/// Parsed Slack event fields.
pub struct SlackEvent {
    pub event_id: String,
    pub event_type: String,
    pub channel: String,
    pub user: String,
    pub text: String,
    pub thread_ts: Option<String>,
    pub ts: String,
}

/// Data gathered from synchronous DB lookups, needed for async processing.
struct DispatchContext {
    tenant_id: uuid::Uuid,
    agent: Agent,
    capabilities: Vec<Capability>,
    conversation: Conversation,
    api_key: String,
    slack_access_token: String,
    #[allow(dead_code)]
    thread_ref: String,
}

/// Route message/app_mention events to the right agent.
pub async fn dispatch_event(
    state: &Arc<super::super::AppState>,
    event: &SlackEvent,
) -> anyhow::Result<()> {
    // Only handle message and app_mention events
    match event.event_type.as_str() {
        "message" | "app_mention" => {}
        _ => return Ok(()),
    }

    // Skip bot messages (no user field or subtypes like bot_message)
    if event.user.is_empty() {
        return Ok(());
    }

    // Gather all DB context synchronously (rusqlite::Connection is not Send)
    let event_id = event.event_id.clone();
    let channel = event.channel.clone();
    let user = event.user.clone();
    let thread_ts = event.thread_ts.clone();
    let ts = event.ts.clone();
    let state_clone = state.clone();

    // Phase 1: DB lookups (blocking)
    let ctx = tokio::task::spawn_blocking(move || -> anyhow::Result<Option<DispatchContext>> {
        // 1. Find binding across tenants
        let tenants = {
            let conn = state_clone.platform_store.conn();
            Tenant::list(&conn)?
        };

        let mut found_binding: Option<(uuid::Uuid, ChatSurfaceBinding)> = None;

        for tenant in &tenants {
            if tenant.status != "active" {
                continue;
            }
            let tenant_store = state_clone.open_tenant_store(&tenant.id)?;
            let conn = tenant_store.conn();
            if let Some(binding) =
                ChatSurfaceBinding::get_by_surface_ref(conn, tenant.id, "slack", &channel)?
            {
                found_binding = Some((tenant.id, binding));
                break;
            }
        }

        // FR-015: If no binding, ignore silently
        let (tenant_id, binding) = match found_binding {
            Some(b) => b,
            None => return Ok(None),
        };

        // 2. Check dedup
        let tenant_store = state_clone.open_tenant_store(&tenant_id)?;
        let conn = tenant_store.conn();
        let is_new = state_clone
            .dedup
            .check_and_record(conn, &event_id, &tenant_id, "slack")?;
        if !is_new {
            tracing::debug!(event_id = %event_id, "Duplicate Slack event, skipping");
            return Ok(None);
        }

        // 3. Load agent
        let agent = Agent::get_current(conn, tenant_id, binding.agent_id)?
            .ok_or_else(|| anyhow::anyhow!("Agent {} not found", binding.agent_id))?;

        if agent.status != "active" {
            return Ok(None);
        }

        // 4. Load capabilities
        let capabilities = Capability::list_by_agent(conn, tenant_id, agent.id)?;

        // 5. Get or create conversation
        let thread_ref_val = thread_ts.as_deref().unwrap_or(&ts);
        let surface_key = format!("{}:{}", channel, thread_ref_val);

        let conversation = match Conversation::get_active_by_surface(conn, tenant_id, &surface_key)?
        {
            Some(conv) => conv,
            None => Conversation::create(
                conn,
                tenant_id,
                agent.id,
                "slack",
                &surface_key,
                &user,
                Some(thread_ref_val),
            )?,
        };

        // 6. Resolve LLM credential
        let credential_name = if agent.model_id.starts_with("claude-") {
            "anthropic"
        } else {
            "openai"
        };

        let api_key =
            match CredentialVaultEntry::get_by_name(conn, tenant_id, credential_name, None)? {
                Some(entry) => {
                    let decrypted = state_clone.vault.decrypt(&entry.encrypted_value)?;
                    String::from_utf8(decrypted)?
                }
                None => {
                    tracing::warn!(
                        tenant_id = %tenant_id,
                        credential = credential_name,
                        "No LLM credential found for tenant"
                    );
                    return Ok(None);
                }
            };

        let slack_access_token = match super::resolve_access_token(&state_clone, conn, tenant_id)? {
            Some(token) => token.value,
            None => {
                tracing::warn!(
                    tenant_id = %tenant_id,
                    "No Slack access token found for tenant"
                );
                return Ok(None);
            }
        };

        Ok(Some(DispatchContext {
            tenant_id,
            agent,
            capabilities,
            conversation,
            api_key,
            slack_access_token,
            thread_ref: thread_ref_val.to_string(),
        }))
    })
    .await??;

    let ctx = match ctx {
        Some(c) => c,
        None => return Ok(()),
    };

    // Phase 2: LLM call (async)
    let provider = crate::llm::resolve_provider(&ctx.agent.model_id, &ctx.api_key)?;

    let invocation = AgentInvocation {
        agent: ctx.agent,
        capabilities: ctx.capabilities,
        conversation_id: ctx.conversation.id,
        tenant_id: ctx.tenant_id,
        user_identity: event.user.clone(),
    };

    // The agent loop needs a Connection, so run it in spawn_blocking too
    let data_dir2 = state.data_dir.clone();
    let tenant_id = ctx.tenant_id;
    let text_clone = event.text.clone();

    let result =
        tokio::task::spawn_blocking(move || -> anyhow::Result<crate::engine::InvocationResult> {
            let tenant_store =
                crate::store::TenantStore::open(std::path::Path::new(&data_dir2), &tenant_id)?;
            let conn = tenant_store.conn();

            // We need a runtime handle to run async LLM calls inside spawn_blocking
            let rt = tokio::runtime::Handle::current();
            rt.block_on(run_agent_loop(
                &invocation,
                provider.as_ref(),
                conn,
                &text_clone,
            ))
        })
        .await??;

    // Phase 3: Post reply via Slack API (async)
    let surface = SlackSurface::new(&ctx.slack_access_token);
    let reply_thread = event.thread_ts.as_deref().unwrap_or(&event.ts);
    surface
        .send_message(&event.channel, Some(reply_thread), &result.response)
        .await?;

    Ok(())
}
