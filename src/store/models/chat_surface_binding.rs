use anyhow::Result;
use rusqlite::{params, Connection};
use uuid::Uuid;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChatSurfaceBinding {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub agent_id: Uuid,
    pub surface_type: String,
    pub surface_ref: String,
    pub created_at: String,
}

fn row_to_binding(row: &rusqlite::Row) -> rusqlite::Result<ChatSurfaceBinding> {
    Ok(ChatSurfaceBinding {
        id: row.get::<_, String>(0)?.parse().unwrap(),
        tenant_id: row.get::<_, String>(1)?.parse().unwrap(),
        agent_id: row.get::<_, String>(2)?.parse().unwrap(),
        surface_type: row.get(3)?,
        surface_ref: row.get(4)?,
        created_at: row.get(5)?,
    })
}

const SELECT_COLS: &str = "id, tenant_id, agent_id, surface_type, surface_ref, created_at";

impl ChatSurfaceBinding {
    pub fn create(
        conn: &Connection,
        tenant_id: Uuid,
        agent_id: Uuid,
        surface_type: &str,
        surface_ref: &str,
    ) -> Result<ChatSurfaceBinding> {
        let id = Uuid::new_v4();
        let now = chrono::Utc::now().to_rfc3339();
        let binding = ChatSurfaceBinding {
            id,
            tenant_id,
            agent_id,
            surface_type: surface_type.to_string(),
            surface_ref: surface_ref.to_string(),
            created_at: now,
        };
        conn.execute(
            "INSERT INTO chat_surface_bindings (id, tenant_id, agent_id, surface_type, surface_ref, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                binding.id.to_string(),
                binding.tenant_id.to_string(),
                binding.agent_id.to_string(),
                binding.surface_type,
                binding.surface_ref,
                binding.created_at,
            ],
        )?;
        Ok(binding)
    }

    pub fn get_by_surface_ref(
        conn: &Connection,
        tenant_id: Uuid,
        surface_type: &str,
        surface_ref: &str,
    ) -> Result<Option<ChatSurfaceBinding>> {
        let sql = format!(
            "SELECT {} FROM chat_surface_bindings
             WHERE tenant_id = ?1 AND surface_type = ?2 AND surface_ref = ?3",
            SELECT_COLS
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query_map(
            params![tenant_id.to_string(), surface_type, surface_ref],
            row_to_binding,
        )?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    pub fn list_by_agent(
        conn: &Connection,
        tenant_id: Uuid,
        agent_id: Uuid,
    ) -> Result<Vec<ChatSurfaceBinding>> {
        let sql = format!(
            "SELECT {} FROM chat_surface_bindings
             WHERE tenant_id = ?1 AND agent_id = ?2 ORDER BY created_at",
            SELECT_COLS
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(
            params![tenant_id.to_string(), agent_id.to_string()],
            row_to_binding,
        )?;
        let mut bindings = Vec::new();
        for row in rows {
            bindings.push(row?);
        }
        Ok(bindings)
    }

    pub fn delete(conn: &Connection, id: Uuid) -> Result<()> {
        conn.execute(
            "DELETE FROM chat_surface_bindings WHERE id = ?1",
            params![id.to_string()],
        )?;
        Ok(())
    }
}
