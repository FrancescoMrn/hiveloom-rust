use anyhow::Result;
use rusqlite::{params, Connection};
use uuid::Uuid;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct McpIdentity {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub name: String,
    pub mapped_person_id: Option<String>,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

const SELECT_COLS: &str =
    "id, tenant_id, name, mapped_person_id, status, created_at, updated_at";

fn row_to_mcp_identity(row: &rusqlite::Row) -> rusqlite::Result<McpIdentity> {
    Ok(McpIdentity {
        id: row.get::<_, String>(0)?.parse().unwrap(),
        tenant_id: row.get::<_, String>(1)?.parse().unwrap(),
        name: row.get(2)?,
        mapped_person_id: row.get(3)?,
        status: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

impl McpIdentity {
    pub fn create(
        conn: &Connection,
        tenant_id: Uuid,
        name: &str,
    ) -> Result<McpIdentity> {
        let id = Uuid::new_v4();
        let now = chrono::Utc::now().to_rfc3339();
        let entry = McpIdentity {
            id,
            tenant_id,
            name: name.to_string(),
            mapped_person_id: None,
            status: "active".to_string(),
            created_at: now.clone(),
            updated_at: now,
        };
        conn.execute(
            "INSERT INTO mcp_identities (id, tenant_id, name, mapped_person_id, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                entry.id.to_string(),
                entry.tenant_id.to_string(),
                entry.name,
                entry.mapped_person_id,
                entry.status,
                entry.created_at,
                entry.updated_at,
            ],
        )?;
        Ok(entry)
    }

    pub fn get(conn: &Connection, id: Uuid) -> Result<Option<McpIdentity>> {
        let sql = format!("SELECT {} FROM mcp_identities WHERE id = ?1", SELECT_COLS);
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query_map(params![id.to_string()], row_to_mcp_identity)?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    pub fn list(conn: &Connection, tenant_id: Uuid) -> Result<Vec<McpIdentity>> {
        let sql = format!(
            "SELECT {} FROM mcp_identities WHERE tenant_id = ?1 ORDER BY created_at",
            SELECT_COLS
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![tenant_id.to_string()], row_to_mcp_identity)?;
        let mut entries = Vec::new();
        for row in rows {
            entries.push(row?);
        }
        Ok(entries)
    }

    pub fn map_person(conn: &Connection, id: Uuid, person_id: &str) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE mcp_identities SET mapped_person_id = ?1, updated_at = ?2 WHERE id = ?3",
            params![person_id, now, id.to_string()],
        )?;
        Ok(())
    }

    pub fn unmap_person(conn: &Connection, id: Uuid) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE mcp_identities SET mapped_person_id = NULL, updated_at = ?1 WHERE id = ?2",
            params![now, id.to_string()],
        )?;
        Ok(())
    }

    pub fn update_status(conn: &Connection, id: Uuid, status: &str) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE mcp_identities SET status = ?1, updated_at = ?2 WHERE id = ?3",
            params![status, now, id.to_string()],
        )?;
        Ok(())
    }

    pub fn revoke(conn: &Connection, id: Uuid) -> Result<()> {
        Self::update_status(conn, id, "revoked")
    }

    pub fn delete(conn: &Connection, id: Uuid) -> Result<()> {
        conn.execute(
            "DELETE FROM mcp_identities WHERE id = ?1",
            params![id.to_string()],
        )?;
        Ok(())
    }
}
