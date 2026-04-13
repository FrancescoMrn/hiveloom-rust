//! CompactionEvent model — immutable audit record of compaction occurrences.

use anyhow::Result;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionEvent {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub agent_id: Uuid,
    pub conversation_id: Uuid,
    pub timestamp: String,
    pub tokens_before: i64,
    pub tokens_after: i64,
    pub strategy: String,
    pub fallback_used: bool,
    pub summary_token_count: Option<i64>,
    pub error_message: Option<String>,
}

impl CompactionEvent {
    /// Create and persist a new CompactionEvent (T017).
    pub fn create(
        conn: &Connection,
        tenant_id: Uuid,
        agent_id: Uuid,
        conversation_id: Uuid,
        tokens_before: i64,
        tokens_after: i64,
        strategy: &str,
        fallback_used: bool,
        summary_token_count: Option<i64>,
        error_message: Option<&str>,
    ) -> Result<Self> {
        let id = Uuid::new_v4();
        let now = chrono::Utc::now().to_rfc3339();

        conn.execute(
            "INSERT INTO compaction_events (id, tenant_id, agent_id, conversation_id,
             timestamp, tokens_before, tokens_after, strategy, fallback_used,
             summary_token_count, error_message)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                id.to_string(),
                tenant_id.to_string(),
                agent_id.to_string(),
                conversation_id.to_string(),
                now,
                tokens_before,
                tokens_after,
                strategy,
                fallback_used,
                summary_token_count,
                error_message,
            ],
        )?;

        Ok(Self {
            id,
            tenant_id,
            agent_id,
            conversation_id,
            timestamp: now,
            tokens_before,
            tokens_after,
            strategy: strategy.to_string(),
            fallback_used,
            summary_token_count,
            error_message: error_message.map(|s| s.to_string()),
        })
    }

    /// List compaction events with filters (T022).
    pub fn list(
        conn: &Connection,
        tenant_id: Uuid,
        agent_id: Option<Uuid>,
        since: Option<&str>,
        limit: i64,
    ) -> Result<Vec<Self>> {
        let mut sql = String::from(
            "SELECT id, tenant_id, agent_id, conversation_id, timestamp,
                    tokens_before, tokens_after, strategy, fallback_used,
                    summary_token_count, error_message
             FROM compaction_events
             WHERE tenant_id = ?1",
        );
        let mut param_count = 1;

        if agent_id.is_some() {
            param_count += 1;
            sql.push_str(&format!(" AND agent_id = ?{}", param_count));
        }
        if since.is_some() {
            param_count += 1;
            sql.push_str(&format!(" AND timestamp >= ?{}", param_count));
        }

        sql.push_str(" ORDER BY timestamp DESC");
        param_count += 1;
        sql.push_str(&format!(" LIMIT ?{}", param_count));

        let mut stmt = conn.prepare(&sql)?;

        // Build params dynamically
        let mut p: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(tenant_id.to_string())];
        if let Some(aid) = agent_id {
            p.push(Box::new(aid.to_string()));
        }
        if let Some(s) = since {
            p.push(Box::new(s.to_string()));
        }
        p.push(Box::new(limit));

        let param_refs: Vec<&dyn rusqlite::types::ToSql> = p.iter().map(|b| b.as_ref()).collect();

        let rows = stmt.query_map(param_refs.as_slice(), |row| {
            let agent_id_str: String = row.get(2)?;
            Ok(CompactionEvent {
                id: row.get::<_, String>(0)?.parse().unwrap(),
                tenant_id: row.get::<_, String>(1)?.parse().unwrap(),
                agent_id: agent_id_str.parse().unwrap(),
                conversation_id: row.get::<_, String>(3)?.parse().unwrap(),
                timestamp: row.get(4)?,
                tokens_before: row.get(5)?,
                tokens_after: row.get(6)?,
                strategy: row.get(7)?,
                fallback_used: row.get(8)?,
                summary_token_count: row.get(9)?,
                error_message: row.get(10)?,
            })
        })?;

        let mut events = Vec::new();
        for row in rows {
            events.push(row?);
        }
        Ok(events)
    }

    /// Count compaction events for an agent (for `hiveloom top`).
    pub fn count_for_agent(conn: &Connection, tenant_id: Uuid, agent_id: Uuid) -> Result<i64> {
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM compaction_events WHERE tenant_id = ?1 AND agent_id = ?2",
            params![tenant_id.to_string(), agent_id.to_string()],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    /// Get the most recent compaction event for an agent (for `hiveloom top`).
    pub fn last_for_agent(
        conn: &Connection,
        tenant_id: Uuid,
        agent_id: Uuid,
    ) -> Result<Option<String>> {
        let mut stmt = conn.prepare(
            "SELECT timestamp FROM compaction_events
             WHERE tenant_id = ?1 AND agent_id = ?2
             ORDER BY timestamp DESC LIMIT 1",
        )?;
        let mut rows = stmt.query_map(
            params![tenant_id.to_string(), agent_id.to_string()],
            |row| row.get::<_, String>(0),
        )?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    /// Check if any recent compaction used fallback truncation (for health alarms T038).
    pub fn has_recent_fallback(
        conn: &Connection,
        tenant_id: Uuid,
        agent_id: Uuid,
        since: &str,
    ) -> Result<bool> {
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM compaction_events
             WHERE tenant_id = ?1 AND agent_id = ?2 AND fallback_used = 1 AND timestamp >= ?3",
            params![tenant_id.to_string(), agent_id.to_string(), since],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    /// Delete compaction events older than a cutoff (T034: 30-day retention).
    pub fn cleanup_expired(conn: &Connection, cutoff: &str) -> Result<usize> {
        let count = conn.execute(
            "DELETE FROM compaction_events WHERE timestamp < ?1",
            params![cutoff],
        )?;
        Ok(count)
    }
}
