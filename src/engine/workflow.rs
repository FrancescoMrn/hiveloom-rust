use crate::compaction::engine::compact_on_resume;
use crate::store::models::Conversation;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowState {
    pub current_step: String,
    pub completed_steps: Vec<String>,
    pub pending_steps: Vec<String>,
    pub intermediate_results: serde_json::Value,
    pub waiting_for: Option<WaitCondition>,
    pub paused_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WaitCondition {
    UserInput,
    OAuthCallback { state_token: String },
    ExternalEvent { event_type: String },
    Schedule { fire_at: String },
}

/// Pause a workflow: serialize state to conversation.workflow_state
pub fn pause_workflow(
    conn: &rusqlite::Connection,
    conversation_id: &uuid::Uuid,
    state: &WorkflowState,
) -> anyhow::Result<()> {
    let mut state = state.clone();
    state.paused_at = Some(chrono::Utc::now().to_rfc3339());
    let json = serde_json::to_string(&state)?;
    Conversation::set_workflow_state(conn, *conversation_id, &json)?;
    Ok(())
}

/// T029: Resume a workflow with compaction check. When a paused conversation
/// is rehydrated and its context exceeds the threshold, trigger compaction
/// before the next LLM call.
pub async fn resume_workflow_with_compaction(
    conn: &rusqlite::Connection,
    conversation_id: &uuid::Uuid,
    provider: &dyn crate::llm::provider::LlmProvider,
    tenant_id: uuid::Uuid,
    agent_id: uuid::Uuid,
    system_prompt: &str,
    model_id: &str,
) -> anyhow::Result<Option<WorkflowState>> {
    let state = resume_workflow(conn, conversation_id)?;
    if state.is_some() {
        // Check if compaction is needed after rehydration
        let _outcome = compact_on_resume(
            conn,
            provider,
            tenant_id,
            agent_id,
            *conversation_id,
            system_prompt,
            model_id,
        )
        .await?;
    }
    Ok(state)
}

/// Resume a workflow: load state, clear waiting_for
pub fn resume_workflow(
    conn: &rusqlite::Connection,
    conversation_id: &uuid::Uuid,
) -> anyhow::Result<Option<WorkflowState>> {
    let conv = Conversation::get(conn, *conversation_id)?;
    let conv = match conv {
        Some(c) => c,
        None => return Ok(None),
    };

    let ws_json = match conv.workflow_state {
        Some(ref s) => s.clone(),
        None => return Ok(None),
    };

    let mut state: WorkflowState = serde_json::from_str(&ws_json)?;
    state.waiting_for = None;
    state.paused_at = None;

    // Persist the updated state (cleared waiting_for)
    let json = serde_json::to_string(&state)?;
    Conversation::set_workflow_state(conn, *conversation_id, &json)?;

    Ok(Some(state))
}

/// Sweep paused workflows older than budget_days and mark abandoned (T070).
pub fn sweep_abandoned_workflows(
    conn: &rusqlite::Connection,
    budget_days: i64,
) -> anyhow::Result<usize> {
    let now = chrono::Utc::now();
    let cutoff = now - chrono::Duration::days(budget_days);
    let cutoff_str = cutoff.to_rfc3339();
    let now_str = now.to_rfc3339();

    // Find active conversations whose workflow_state is not null and updated_at
    // is older than the budget cutoff.
    let count = conn.execute(
        "UPDATE conversations
         SET status = 'abandoned', abandoned_at = ?1, updated_at = ?1
         WHERE status = 'active'
           AND workflow_state IS NOT NULL
           AND updated_at < ?2",
        rusqlite::params![now_str, cutoff_str],
    )?;

    Ok(count)
}

/// On startup, find conversations with non-null workflow_state and status=active (T071).
pub fn find_resumable_workflows(
    conn: &rusqlite::Connection,
) -> anyhow::Result<Vec<(uuid::Uuid, WorkflowState)>> {
    let mut stmt = conn.prepare(
        "SELECT id, workflow_state FROM conversations
         WHERE status = 'active' AND workflow_state IS NOT NULL",
    )?;

    let rows = stmt.query_map([], |row| {
        let id_str: String = row.get(0)?;
        let ws_str: String = row.get(1)?;
        Ok((id_str, ws_str))
    })?;

    let mut results = Vec::new();
    for row in rows {
        let (id_str, ws_str) = row?;
        let id: uuid::Uuid = id_str.parse().map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("invalid uuid: {}", e))),
            )
        })?;
        match serde_json::from_str::<WorkflowState>(&ws_str) {
            Ok(state) => results.push((id, state)),
            Err(e) => {
                tracing::warn!(
                    conversation_id = %id,
                    error = %e,
                    "Skipping conversation with invalid workflow_state JSON"
                );
            }
        }
    }

    Ok(results)
}
