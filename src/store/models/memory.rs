use anyhow::Result;
use rusqlite::{params, Connection};
use uuid::Uuid;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MemoryEntry {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub agent_id: Uuid,
    pub scope: String,
    pub key: String,
    pub value: String,
    pub source_conversation_id: Option<String>,
    pub confidence: f64,
    pub coerced: bool,
    pub coerced_from_scope: Option<String>,
    pub archived: bool,
    pub archived_at: Option<String>,
    pub expires_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

const SELECT_COLS: &str =
    "id, tenant_id, agent_id, scope, key, value, source_conversation_id, confidence, coerced, \
     coerced_from_scope, archived, archived_at, expires_at, created_at, updated_at";

fn row_to_memory(row: &rusqlite::Row) -> rusqlite::Result<MemoryEntry> {
    Ok(MemoryEntry {
        id: row.get::<_, String>(0)?.parse().unwrap(),
        tenant_id: row.get::<_, String>(1)?.parse().unwrap(),
        agent_id: row.get::<_, String>(2)?.parse().unwrap(),
        scope: row.get(3)?,
        key: row.get(4)?,
        value: row.get(5)?,
        source_conversation_id: row.get(6)?,
        confidence: row.get(7)?,
        coerced: row.get::<_, i64>(8)? != 0,
        coerced_from_scope: row.get(9)?,
        archived: row.get::<_, i64>(10)? != 0,
        archived_at: row.get(11)?,
        expires_at: row.get(12)?,
        created_at: row.get(13)?,
        updated_at: row.get(14)?,
    })
}

impl MemoryEntry {
    #[allow(clippy::too_many_arguments)]
    pub fn upsert(
        conn: &Connection,
        tenant_id: Uuid,
        agent_id: Uuid,
        scope: &str,
        key: &str,
        value: &str,
        source_conversation_id: Option<&str>,
        confidence: f64,
        coerced: bool,
        coerced_from_scope: Option<&str>,
    ) -> Result<MemoryEntry> {
        let now = chrono::Utc::now().to_rfc3339();
        let id = Uuid::new_v4();
        conn.execute(
            "INSERT INTO memory_entries (id, tenant_id, agent_id, scope, key, value,
             source_conversation_id, confidence, coerced, coerced_from_scope, archived,
             created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 0, ?11, ?11)
             ON CONFLICT(tenant_id, agent_id, scope, key) DO UPDATE SET
               value = excluded.value,
               source_conversation_id = excluded.source_conversation_id,
               confidence = excluded.confidence,
               coerced = excluded.coerced,
               coerced_from_scope = excluded.coerced_from_scope,
               updated_at = excluded.updated_at",
            params![
                id.to_string(),
                tenant_id.to_string(),
                agent_id.to_string(),
                scope,
                key,
                value,
                source_conversation_id,
                confidence,
                coerced as i64,
                coerced_from_scope,
                now,
            ],
        )?;

        // Fetch the actual row (may have existing id if it was an update)
        let sql = format!(
            "SELECT {} FROM memory_entries WHERE tenant_id = ?1 AND agent_id = ?2 AND scope = ?3 AND key = ?4",
            SELECT_COLS
        );
        let mut stmt = conn.prepare(&sql)?;
        let entry = stmt.query_row(
            params![tenant_id.to_string(), agent_id.to_string(), scope, key],
            row_to_memory,
        )?;
        Ok(entry)
    }

    pub fn read_for_user(
        conn: &Connection,
        tenant_id: Uuid,
        agent_id: Uuid,
        user_identity: &str,
    ) -> Result<Vec<MemoryEntry>> {
        // Return tenant-scoped entries + user-scoped entries for the given user identity
        let sql = format!(
            "SELECT {} FROM memory_entries
             WHERE tenant_id = ?1 AND agent_id = ?2 AND archived = 0
               AND (scope = 'tenant' OR scope = ?3)
             ORDER BY key",
            SELECT_COLS
        );
        let user_scope = format!("user:{}", user_identity);
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(
            params![tenant_id.to_string(), agent_id.to_string(), user_scope],
            row_to_memory,
        )?;
        let mut entries = Vec::new();
        for row in rows {
            entries.push(row?);
        }
        Ok(entries)
    }

    pub fn delete(conn: &Connection, id: Uuid) -> Result<()> {
        conn.execute(
            "DELETE FROM memory_entries WHERE id = ?1",
            params![id.to_string()],
        )?;
        Ok(())
    }

    pub fn soft_archive(conn: &Connection, id: Uuid, expires_at: Option<&str>) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE memory_entries SET archived = 1, archived_at = ?1, expires_at = ?2, updated_at = ?1
             WHERE id = ?3",
            params![now, expires_at, id.to_string()],
        )?;
        Ok(())
    }

    pub fn restore(conn: &Connection, id: Uuid) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE memory_entries SET archived = 0, archived_at = NULL, expires_at = NULL, updated_at = ?1
             WHERE id = ?2",
            params![now, id.to_string()],
        )?;
        Ok(())
    }
}
