use anyhow::Result;
use rusqlite::{params, Connection};
use uuid::Uuid;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct McpSetupCode {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub mcp_identity_id: Uuid,
    pub code_hash: String,
    pub expires_at: String,
    pub used_at: Option<String>,
    pub created_at: String,
}

const SELECT_COLS: &str =
    "id, tenant_id, mcp_identity_id, code_hash, expires_at, used_at, created_at";

fn row_to_setup_code(row: &rusqlite::Row) -> rusqlite::Result<McpSetupCode> {
    Ok(McpSetupCode {
        id: row.get::<_, String>(0)?.parse().unwrap(),
        tenant_id: row.get::<_, String>(1)?.parse().unwrap(),
        mcp_identity_id: row.get::<_, String>(2)?.parse().unwrap(),
        code_hash: row.get(3)?,
        expires_at: row.get(4)?,
        used_at: row.get(5)?,
        created_at: row.get(6)?,
    })
}

impl McpSetupCode {
    pub fn create(
        conn: &Connection,
        tenant_id: Uuid,
        mcp_identity_id: Uuid,
        code_hash: &str,
        expires_at: &str,
    ) -> Result<McpSetupCode> {
        let id = Uuid::new_v4();
        let now = chrono::Utc::now().to_rfc3339();
        let entry = McpSetupCode {
            id,
            tenant_id,
            mcp_identity_id,
            code_hash: code_hash.to_string(),
            expires_at: expires_at.to_string(),
            used_at: None,
            created_at: now,
        };
        conn.execute(
            "INSERT INTO mcp_setup_codes
             (id, tenant_id, mcp_identity_id, code_hash, expires_at, used_at, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                entry.id.to_string(),
                entry.tenant_id.to_string(),
                entry.mcp_identity_id.to_string(),
                entry.code_hash,
                entry.expires_at,
                entry.used_at,
                entry.created_at,
            ],
        )?;
        Ok(entry)
    }

    pub fn get(conn: &Connection, id: Uuid) -> Result<Option<McpSetupCode>> {
        let sql = format!("SELECT {} FROM mcp_setup_codes WHERE id = ?1", SELECT_COLS);
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query_map(params![id.to_string()], row_to_setup_code)?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    /// Find a valid (unused, not expired) setup code by its hash.
    pub fn get_valid_by_hash(conn: &Connection, code_hash: &str) -> Result<Option<McpSetupCode>> {
        let now = chrono::Utc::now().to_rfc3339();
        let sql = format!(
            "SELECT {} FROM mcp_setup_codes
             WHERE code_hash = ?1 AND used_at IS NULL AND expires_at > ?2",
            SELECT_COLS
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query_map(params![code_hash, now], row_to_setup_code)?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    pub fn mark_used(conn: &Connection, id: Uuid) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE mcp_setup_codes SET used_at = ?1 WHERE id = ?2",
            params![now, id.to_string()],
        )?;
        Ok(())
    }

    pub fn cleanup_expired(conn: &Connection) -> Result<usize> {
        let now = chrono::Utc::now().to_rfc3339();
        let count = conn.execute(
            "DELETE FROM mcp_setup_codes WHERE expires_at < ?1 AND used_at IS NULL",
            params![now],
        )?;
        Ok(count)
    }

    /// Revoke all unused setup codes for a given identity (used when reissuing).
    pub fn revoke_all_for_identity(conn: &Connection, mcp_identity_id: Uuid) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE mcp_setup_codes SET used_at = ?1
             WHERE mcp_identity_id = ?2 AND used_at IS NULL",
            params![now, mcp_identity_id.to_string()],
        )?;
        Ok(())
    }
}
