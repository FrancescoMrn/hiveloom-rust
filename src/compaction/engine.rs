//! Compaction engine orchestrator.
//!
//! Coordinates threshold checking, summarization, fallback truncation,
//! event recording, and raw turn archival.

use anyhow::Result;
use rusqlite::{params, Connection};
use uuid::Uuid;

use crate::compaction::config::resolve_config;
use crate::compaction::counter::{model_context_window, TokenCounter};
use crate::compaction::event::CompactionEvent;
use crate::compaction::summarizer::Summarizer;
use crate::compaction::truncator::Truncator;
use crate::llm::provider::{LlmProvider, Message};
use crate::store::models::ConversationTurn;

/// Result of a compaction check — either no-op or compaction was performed.
#[derive(Debug)]
pub enum CompactionOutcome {
    /// Token count is below threshold; no compaction needed.
    NotNeeded,
    /// Compaction was performed.
    Compacted {
        tokens_before: usize,
        tokens_after: usize,
        strategy: String,
        fallback_used: bool,
        summary: Option<String>,
        show_indicator: bool,
    },
}

pub struct CompactionEngine;

impl CompactionEngine {
    /// Pre-LLM-call compaction check (T016, T019).
    ///
    /// This is the main entry point, called before each LLM call in the agent loop.
    /// It checks token count against the configured threshold and triggers compaction
    /// if needed.
    ///
    /// T039: This function MUST NOT write to agent persistent memory store.
    pub async fn check_and_compact(
        conn: &Connection,
        provider: &dyn LlmProvider,
        tenant_id: Uuid,
        agent_id: Uuid,
        conversation_id: Uuid,
        system_prompt: &str,
        model_id: &str,
    ) -> Result<CompactionOutcome> {
        // T027: Resolve config fresh on every call (hot-reload)
        let config = resolve_config(conn, tenant_id, agent_id)?;
        let counter = TokenCounter::new();
        let context_window = model_context_window(model_id);

        // Load current turns
        let turns = ConversationTurn::list_by_conversation(conn, conversation_id)?;
        if turns.is_empty() {
            return Ok(CompactionOutcome::NotNeeded);
        }

        // Build messages to count total tokens
        let mut messages = vec![Message {
            role: "system".to_string(),
            content: system_prompt.to_string(),
        }];

        // T021: If there's an existing compacted summary, include it
        let existing_summary = get_compacted_summary(conn, conversation_id)?;
        if let Some(ref summary) = existing_summary {
            messages.push(Message {
                role: "system".to_string(),
                content: format!("[Previous context summary]\n{}", summary),
            });
        }

        for turn in &turns {
            messages.push(Message {
                role: turn.role.clone(),
                content: turn.content.clone(),
            });
        }

        let total_tokens = counter.count_messages(&messages);
        let threshold_tokens = (context_window as f64 * config.threshold_pct as f64 / 100.0) as usize;

        if total_tokens < threshold_tokens {
            return Ok(CompactionOutcome::NotNeeded);
        }

        tracing::info!(
            conversation_id = %conversation_id,
            total_tokens = total_tokens,
            threshold_tokens = threshold_tokens,
            "Context threshold exceeded, triggering compaction"
        );

        // Determine which turns to compact vs. protect
        let protected_count = config.protected_turn_count as usize;
        let (turns_to_compact, _protected_turns) =
            split_turns_for_compaction(&turns, protected_count);

        if turns_to_compact.is_empty() {
            return Ok(CompactionOutcome::NotNeeded);
        }

        let tokens_before = total_tokens;

        // Calculate max summary token budget
        let max_summary_tokens =
            (context_window as f64 * config.max_summary_fraction_pct as f64 / 100.0) as usize;

        // T021: Get workflow state for summary preservation
        let workflow_state = get_workflow_state(conn, conversation_id)?;

        // Try LLM summarization first
        let compaction_result = Summarizer::summarize(
            provider,
            &counter,
            system_prompt,
            &turns_to_compact,
            max_summary_tokens,
            workflow_state.as_deref(),
        )
        .await;

        match compaction_result {
            Ok(summary_result) => {
                // T018: Archive compacted turns
                archive_turns(conn, tenant_id, conversation_id, &turns_to_compact)?;

                // Remove compacted turns from conversation_turns
                remove_compacted_turns(conn, conversation_id, &turns_to_compact)?;

                // T021: Build new summary, incorporating existing summary if present
                let new_summary = if let Some(ref existing) = existing_summary {
                    format!(
                        "{}\n\n---\n\n[Updated summary]\n{}",
                        existing, summary_result.summary
                    )
                } else {
                    summary_result.summary.clone()
                };

                // Update conversation context with new summary
                update_compaction_state(conn, conversation_id, &new_summary)?;

                // Recalculate tokens after compaction
                let tokens_after = calculate_tokens_after(
                    conn,
                    conversation_id,
                    system_prompt,
                    &new_summary,
                    &counter,
                )?;

                // T017: Record compaction event
                CompactionEvent::create(
                    conn,
                    tenant_id,
                    agent_id,
                    conversation_id,
                    tokens_before as i64,
                    tokens_after as i64,
                    "summarization",
                    false,
                    Some(summary_result.summary_token_count as i64),
                    None,
                )?;

                Ok(CompactionOutcome::Compacted {
                    tokens_before,
                    tokens_after,
                    strategy: "summarization".to_string(),
                    fallback_used: false,
                    summary: Some(new_summary),
                    show_indicator: config.show_indicator,
                })
            }
            Err(err) => {
                tracing::warn!(
                    conversation_id = %conversation_id,
                    error = %err,
                    "Summarization failed, falling back to truncation"
                );

                // T015: Fallback truncation
                let truncation = Truncator::truncate(
                    &turns,
                    &counter,
                    context_window,
                    protected_count,
                    existing_summary.as_deref(),
                );

                // Archive dropped turns
                let dropped_turns: Vec<ConversationTurn> = turns
                    .iter()
                    .filter(|t| truncation.dropped_turn_indices.contains(&t.turn_index))
                    .cloned()
                    .collect();
                archive_turns(conn, tenant_id, conversation_id, &dropped_turns)?;

                // Remove dropped turns
                remove_compacted_turns(conn, conversation_id, &dropped_turns)?;

                // Update compaction count
                increment_compaction_count(conn, conversation_id)?;

                let tokens_after = truncation.tokens_after;

                // T017: Record compaction event with fallback
                CompactionEvent::create(
                    conn,
                    tenant_id,
                    agent_id,
                    conversation_id,
                    tokens_before as i64,
                    tokens_after as i64,
                    "truncation",
                    true,
                    None,
                    Some(&err.to_string()),
                )?;

                Ok(CompactionOutcome::Compacted {
                    tokens_before,
                    tokens_after,
                    strategy: "truncation".to_string(),
                    fallback_used: true,
                    summary: existing_summary,
                    show_indicator: config.show_indicator,
                })
            }
        }
    }
}

/// T020: Split turns into compactable and protected sets.
///
/// Protected turns: system prompt turns, most recent N turns,
/// in-flight tool_call/tool_result pairs within the protected window.
fn split_turns_for_compaction(
    turns: &[ConversationTurn],
    protected_count: usize,
) -> (Vec<ConversationTurn>, Vec<ConversationTurn>) {
    if turns.len() <= protected_count {
        return (vec![], turns.to_vec());
    }

    let split_point = turns.len() - protected_count;
    let mut to_compact: Vec<ConversationTurn> = Vec::new();
    let mut protected: Vec<ConversationTurn> = Vec::new();

    for (i, turn) in turns.iter().enumerate() {
        // System prompt is never compacted (T020)
        if turn.role == "system" {
            protected.push(turn.clone());
            continue;
        }

        if i >= split_point {
            protected.push(turn.clone());
        } else {
            to_compact.push(turn.clone());
        }
    }

    // Ensure in-flight tool_call/tool_result pairs stay together (T020)
    // If the last compacted turn is an assistant tool_use and the first protected
    // is a tool_result, move the tool_use to protected
    if let Some(last) = to_compact.last() {
        if last.content.starts_with("tool_use:") {
            if let Some(first_protected) = protected.iter().find(|t| t.role != "system") {
                if first_protected.role == "tool_result" {
                    let moved = to_compact.pop().unwrap();
                    protected.insert(0, moved);
                }
            }
        }
    }

    (to_compact, protected)
}

// ── Database helpers ───────────────────────────────────────────────────

/// T018: Move compacted turns to raw_turn_archive.
fn archive_turns(
    conn: &Connection,
    tenant_id: Uuid,
    conversation_id: Uuid,
    turns: &[ConversationTurn],
) -> Result<()> {
    let now = chrono::Utc::now();
    let compacted_at = now.to_rfc3339();
    let expires_at = (now + chrono::Duration::days(30)).to_rfc3339();

    for turn in turns {
        let id = Uuid::new_v4();
        conn.execute(
            "INSERT INTO raw_turn_archive (id, tenant_id, conversation_id, turn_index,
             role, content, token_count, compacted_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                id.to_string(),
                tenant_id.to_string(),
                conversation_id.to_string(),
                turn.turn_index,
                turn.role,
                turn.content,
                turn.token_count,
                compacted_at,
                expires_at,
            ],
        )?;
    }
    Ok(())
}

/// Remove compacted turns from the active conversation_turns table.
fn remove_compacted_turns(
    conn: &Connection,
    conversation_id: Uuid,
    turns: &[ConversationTurn],
) -> Result<()> {
    for turn in turns {
        conn.execute(
            "DELETE FROM conversation_turns WHERE id = ?1 AND conversation_id = ?2",
            params![turn.id.to_string(), conversation_id.to_string()],
        )?;
    }
    Ok(())
}

/// Get existing compacted summary from conversation.
fn get_compacted_summary(conn: &Connection, conversation_id: Uuid) -> Result<Option<String>> {
    let result: Option<String> = conn
        .query_row(
            "SELECT compacted_summary FROM conversations WHERE id = ?1",
            params![conversation_id.to_string()],
            |row| row.get(0),
        )
        .unwrap_or(None);
    Ok(result)
}

/// Get workflow state from conversation.
fn get_workflow_state(conn: &Connection, conversation_id: Uuid) -> Result<Option<String>> {
    let result: Option<String> = conn
        .query_row(
            "SELECT workflow_state FROM conversations WHERE id = ?1",
            params![conversation_id.to_string()],
            |row| row.get(0),
        )
        .unwrap_or(None);
    Ok(result)
}

/// Update conversation with compaction state.
fn update_compaction_state(
    conn: &Connection,
    conversation_id: Uuid,
    summary: &str,
) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE conversations
         SET compacted_summary = ?1,
             compaction_count = COALESCE(compaction_count, 0) + 1,
             last_compaction_at = ?2,
             raw_turns_archived = 1,
             updated_at = ?2
         WHERE id = ?3",
        params![summary, now, conversation_id.to_string()],
    )?;
    Ok(())
}

/// Increment compaction count without updating summary (used for truncation fallback).
fn increment_compaction_count(conn: &Connection, conversation_id: Uuid) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE conversations
         SET compaction_count = COALESCE(compaction_count, 0) + 1,
             last_compaction_at = ?1,
             raw_turns_archived = 1,
             updated_at = ?1
         WHERE id = ?2",
        params![now, conversation_id.to_string()],
    )?;
    Ok(())
}

/// Calculate token count after compaction.
fn calculate_tokens_after(
    conn: &Connection,
    conversation_id: Uuid,
    system_prompt: &str,
    summary: &str,
    counter: &TokenCounter,
) -> Result<usize> {
    let remaining_turns = ConversationTurn::list_by_conversation(conn, conversation_id)?;
    let mut messages = vec![
        Message {
            role: "system".to_string(),
            content: system_prompt.to_string(),
        },
        Message {
            role: "system".to_string(),
            content: format!("[Previous context summary]\n{}", summary),
        },
    ];
    for turn in &remaining_turns {
        messages.push(Message {
            role: turn.role.clone(),
            content: turn.content.clone(),
        });
    }
    Ok(counter.count_messages(&messages))
}

/// T033: Clean up expired raw turn archive entries (30-day retention).
pub fn cleanup_expired_archives(conn: &Connection) -> Result<usize> {
    let now = chrono::Utc::now().to_rfc3339();
    let count = conn.execute(
        "DELETE FROM raw_turn_archive WHERE expires_at < ?1",
        params![now],
    )?;
    Ok(count)
}

/// T029: Compaction-on-resume — check if a rehydrated conversation needs compaction.
pub async fn compact_on_resume(
    conn: &Connection,
    provider: &dyn LlmProvider,
    tenant_id: Uuid,
    agent_id: Uuid,
    conversation_id: Uuid,
    system_prompt: &str,
    model_id: &str,
) -> Result<CompactionOutcome> {
    CompactionEngine::check_and_compact(
        conn,
        provider,
        tenant_id,
        agent_id,
        conversation_id,
        system_prompt,
        model_id,
    )
    .await
}

/// T037: Verify tenant isolation — all compaction reads are tenant-scoped.
/// This is enforced by the WHERE tenant_id = ?1 clause in all queries in
/// CompactionEvent, CompactionConfig, and raw_turn_archive reads.
/// This function exists as a documentation marker and can be called in tests
/// to verify isolation.
pub fn verify_tenant_isolation(conn: &Connection, tenant_id: Uuid) -> Result<bool> {
    // Verify compaction_config is tenant-scoped
    let config_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM compaction_config WHERE tenant_id != ?1",
        params![tenant_id.to_string()],
        |row| row.get(0),
    )?;

    // Verify compaction_events is tenant-scoped
    let event_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM compaction_events WHERE tenant_id != ?1",
        params![tenant_id.to_string()],
        |row| row.get(0),
    )?;

    // Verify raw_turn_archive is tenant-scoped
    let archive_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM raw_turn_archive WHERE tenant_id != ?1",
        params![tenant_id.to_string()],
        |row| row.get(0),
    )?;

    // In a properly isolated per-tenant database, all counts should be 0
    // because each tenant has its own database file.
    Ok(config_count == 0 && event_count == 0 && archive_count == 0)
}
