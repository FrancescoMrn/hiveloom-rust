use crate::store::models::Conversation;

/// Find an active conversation for the given surface or create a new one.
///
/// If a `thread_ref` is provided, looks for an active conversation matching that
/// thread. Otherwise, matches on the surface_ref alone. If no active
/// conversation is found, a new one is created.
pub fn get_or_create_conversation(
    conn: &rusqlite::Connection,
    tenant_id: &uuid::Uuid,
    agent_id: &uuid::Uuid,
    surface_type: &str,
    surface_ref: &str,
    user_identity: &str,
    thread_ref: Option<&str>,
) -> anyhow::Result<Conversation> {
    // Try to find an existing active conversation for this surface
    if let Some(existing) = Conversation::get_active_by_surface(conn, *tenant_id, surface_ref)? {
        // If thread_ref is specified, verify it matches (or is absent on both sides)
        let thread_matches = match (thread_ref, &existing.thread_ref) {
            (Some(wanted), Some(existing_ref)) => wanted == existing_ref,
            (None, None) => true,
            _ => false,
        };
        if thread_matches {
            return Ok(existing);
        }
    }

    // No matching active conversation found -- create a new one
    Conversation::create(
        conn,
        *tenant_id,
        *agent_id,
        surface_type,
        surface_ref,
        user_identity,
        thread_ref,
    )
}

/// Mark a conversation as concluded after idle timeout.
pub fn conclude_conversation(conn: &rusqlite::Connection, id: &uuid::Uuid) -> anyhow::Result<()> {
    Conversation::update_status(conn, *id, "concluded")
}

/// Mark a conversation as abandoned after workflow budget is exhausted.
pub fn abandon_conversation(conn: &rusqlite::Connection, id: &uuid::Uuid) -> anyhow::Result<()> {
    Conversation::update_status(conn, *id, "abandoned")
}
