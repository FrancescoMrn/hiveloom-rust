use anyhow::Result;
use rusqlite::{params, Connection};
use uuid::Uuid;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct McpClientRegistration {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub mcp_identity_id: Uuid,
    pub client_id: String,
    pub access_token_hash: String,
    pub refresh_token_hash: Option<String>,
    pub token_expires_at: Option<String>,
    pub created_at: String,
    pub revoked_at: Option<String>,
}

const SELECT_COLS: &str =
    "id, tenant_id, mcp_identity_id, client_id, access_token_hash, refresh_token_hash, \
     token_expires_at, created_at, revoked_at";

fn row_to_registration(row: &rusqlite::Row) -> rusqlite::Result<McpClientRegistration> {
    Ok(McpClientRegistration {
        id: row.get::<_, String>(0)?.parse().unwrap(),
        tenant_id: row.get::<_, String>(1)?.parse().unwrap(),
        mcp_identity_id: row.get::<_, String>(2)?.parse().unwrap(),
        client_id: row.get(3)?,
        access_token_hash: row.get(4)?,
        refresh_token_hash: row.get(5)?,
        token_expires_at: row.get(6)?,
        created_at: row.get(7)?,
        revoked_at: row.get(8)?,
    })
}

impl McpClientRegistration {
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        conn: &Connection,
        tenant_id: Uuid,
        mcp_identity_id: Uuid,
        client_id: &str,
        access_token_hash: &str,
        refresh_token_hash: Option<&str>,
        token_expires_at: Option<&str>,
    ) -> Result<McpClientRegistration> {
        let id = Uuid::new_v4();
        let now = chrono::Utc::now().to_rfc3339();
        let entry = McpClientRegistration {
            id,
            tenant_id,
            mcp_identity_id,
            client_id: client_id.to_string(),
            access_token_hash: access_token_hash.to_string(),
            refresh_token_hash: refresh_token_hash.map(|s| s.to_string()),
            token_expires_at: token_expires_at.map(|s| s.to_string()),
            created_at: now,
            revoked_at: None,
        };
        conn.execute(
            "INSERT INTO mcp_client_registrations
             (id, tenant_id, mcp_identity_id, client_id, access_token_hash,
              refresh_token_hash, token_expires_at, created_at, revoked_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                entry.id.to_string(),
                entry.tenant_id.to_string(),
                entry.mcp_identity_id.to_string(),
                entry.client_id,
                entry.access_token_hash,
                entry.refresh_token_hash,
                entry.token_expires_at,
                entry.created_at,
                entry.revoked_at,
            ],
        )?;
        Ok(entry)
    }

    pub fn get(conn: &Connection, id: Uuid) -> Result<Option<McpClientRegistration>> {
        let sql = format!(
            "SELECT {} FROM mcp_client_registrations WHERE id = ?1",
            SELECT_COLS
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query_map(params![id.to_string()], row_to_registration)?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    pub fn get_by_client_id(
        conn: &Connection,
        client_id: &str,
    ) -> Result<Option<McpClientRegistration>> {
        let sql = format!(
            "SELECT {} FROM mcp_client_registrations WHERE client_id = ?1 AND revoked_at IS NULL",
            SELECT_COLS
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query_map(params![client_id], row_to_registration)?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    pub fn get_by_access_token_hash(
        conn: &Connection,
        access_token_hash: &str,
    ) -> Result<Option<McpClientRegistration>> {
        let sql = format!(
            "SELECT {} FROM mcp_client_registrations
             WHERE access_token_hash = ?1 AND revoked_at IS NULL",
            SELECT_COLS
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query_map(params![access_token_hash], row_to_registration)?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    pub fn list_by_identity(
        conn: &Connection,
        mcp_identity_id: Uuid,
    ) -> Result<Vec<McpClientRegistration>> {
        let sql = format!(
            "SELECT {} FROM mcp_client_registrations
             WHERE mcp_identity_id = ?1 ORDER BY created_at",
            SELECT_COLS
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![mcp_identity_id.to_string()], row_to_registration)?;
        let mut entries = Vec::new();
        for row in rows {
            entries.push(row?);
        }
        Ok(entries)
    }

    pub fn revoke(conn: &Connection, id: Uuid) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE mcp_client_registrations SET revoked_at = ?1 WHERE id = ?2",
            params![now, id.to_string()],
        )?;
        Ok(())
    }

    pub fn update_tokens(
        conn: &Connection,
        id: Uuid,
        access_token_hash: &str,
        refresh_token_hash: Option<&str>,
        token_expires_at: Option<&str>,
    ) -> Result<()> {
        conn.execute(
            "UPDATE mcp_client_registrations
             SET access_token_hash = ?1, refresh_token_hash = ?2, token_expires_at = ?3
             WHERE id = ?4",
            params![
                access_token_hash,
                refresh_token_hash,
                token_expires_at,
                id.to_string()
            ],
        )?;
        Ok(())
    }

    pub fn delete(conn: &Connection, id: Uuid) -> Result<()> {
        conn.execute(
            "DELETE FROM mcp_client_registrations WHERE id = ?1",
            params![id.to_string()],
        )?;
        Ok(())
    }
}
