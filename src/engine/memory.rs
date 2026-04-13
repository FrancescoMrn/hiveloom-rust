use crate::store::models::{Agent, MemoryEntry};

/// Read memory entries respecting scope: returns both tenant-scoped and
/// user-scoped entries for the given user identity.
pub fn read_memories(
    conn: &rusqlite::Connection,
    tenant_id: &uuid::Uuid,
    agent_id: &uuid::Uuid,
    user_identity: &str,
) -> anyhow::Result<Vec<MemoryEntry>> {
    MemoryEntry::read_for_user(conn, *tenant_id, *agent_id, user_identity)
}

/// Write a memory entry with scope enforcement based on the agent's scope_mode.
///
/// Scope modes:
/// - `dual`: both tenant and user scopes are allowed.
/// - `tenant-only`: only tenant scope is allowed; user-scoped writes are either
///   coerced to tenant scope or dropped (based on scope_coerce_policy).
/// - `user-only`: only user scope is allowed; tenant-scoped writes are either
///   coerced to user scope or dropped.
pub fn write_memory(
    conn: &rusqlite::Connection,
    agent: &Agent,
    user_identity: &str,
    key: &str,
    value: &str,
    source_conversation_id: Option<&uuid::Uuid>,
) -> anyhow::Result<()> {
    let source_conv_str = source_conversation_id.map(|id| id.to_string());
    let source_conv_ref = source_conv_str.as_deref();

    // Determine the target scope based on the agent's default_scope_policy
    let requested_scope = match agent.default_scope_policy.as_str() {
        "tenant" => "tenant".to_string(),
        "user" => format!("user:{}", user_identity),
        _ => "tenant".to_string(),
    };

    // Enforce scope_mode restrictions
    let is_tenant_scope = requested_scope == "tenant";
    let (final_scope, coerced, coerced_from) = match agent.scope_mode.as_str() {
        "dual" => {
            // Both scopes allowed, no coercion needed
            (requested_scope, false, None)
        }
        "tenant-only" => {
            if is_tenant_scope {
                (requested_scope, false, None)
            } else {
                // User-scoped write, need to handle based on coerce policy
                match agent.scope_coerce_policy.as_str() {
                    "coerce" => ("tenant".to_string(), true, Some(requested_scope)),
                    "drop" => return Ok(()), // silently drop
                    _ => return Ok(()),
                }
            }
        }
        "user-only" => {
            if !is_tenant_scope {
                (requested_scope, false, None)
            } else {
                // Tenant-scoped write, need to handle based on coerce policy
                match agent.scope_coerce_policy.as_str() {
                    "coerce" => (
                        format!("user:{}", user_identity),
                        true,
                        Some("tenant".to_string()),
                    ),
                    "drop" => return Ok(()), // silently drop
                    _ => return Ok(()),
                }
            }
        }
        _ => (requested_scope, false, None),
    };

    MemoryEntry::upsert(
        conn,
        agent.tenant_id,
        agent.id,
        &final_scope,
        key,
        value,
        source_conv_ref,
        1.0,
        coerced,
        coerced_from.as_deref(),
    )?;

    Ok(())
}

/// Promote a user-scoped memory entry to tenant scope.
pub fn promote_to_tenant(
    conn: &rusqlite::Connection,
    entry_id: &uuid::Uuid,
    tenant_id: &uuid::Uuid,
    agent_id: &uuid::Uuid,
) -> anyhow::Result<()> {
    // Read the existing entry to get its key/value
    let entries = MemoryEntry::read_for_user(conn, *tenant_id, *agent_id, "")?;
    let entry = entries
        .iter()
        .find(|e| e.id == *entry_id)
        .ok_or_else(|| anyhow::anyhow!("Memory entry not found: {}", entry_id))?;

    if entry.scope == "tenant" {
        // Already tenant-scoped, nothing to do
        return Ok(());
    }

    // Create a new tenant-scoped entry with the same key/value
    MemoryEntry::upsert(
        conn,
        *tenant_id,
        *agent_id,
        "tenant",
        &entry.key,
        &entry.value,
        entry.source_conversation_id.as_deref(),
        entry.confidence,
        true,
        Some(&entry.scope),
    )?;

    // Archive the original user-scoped entry
    MemoryEntry::soft_archive(conn, *entry_id, None)?;

    Ok(())
}
