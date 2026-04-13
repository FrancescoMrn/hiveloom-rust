use anyhow::Result;
use rusqlite::{params, Connection};
use uuid::Uuid;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Capability {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub agent_id: Uuid,
    pub name: String,
    pub description: String,
    pub endpoint_url: String,
    pub auth_type: String,
    pub credential_ref: Option<String>,
    pub input_schema: Option<String>,
    pub output_schema: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

const SELECT_COLS: &str =
    "id, tenant_id, agent_id, name, description, endpoint_url, auth_type, credential_ref, \
     input_schema, output_schema, created_at, updated_at";

fn row_to_capability(row: &rusqlite::Row) -> rusqlite::Result<Capability> {
    Ok(Capability {
        id: row.get::<_, String>(0)?.parse().unwrap(),
        tenant_id: row.get::<_, String>(1)?.parse().unwrap(),
        agent_id: row.get::<_, String>(2)?.parse().unwrap(),
        name: row.get(3)?,
        description: row.get(4)?,
        endpoint_url: row.get(5)?,
        auth_type: row.get(6)?,
        credential_ref: row.get(7)?,
        input_schema: row.get(8)?,
        output_schema: row.get(9)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
    })
}

/// Parameters for creating a new capability (avoids too-many-arguments lint).
pub struct CreateCapabilityParams<'a> {
    pub tenant_id: Uuid,
    pub agent_id: Uuid,
    pub name: &'a str,
    pub description: &'a str,
    pub endpoint_url: &'a str,
    pub auth_type: &'a str,
    pub credential_ref: Option<&'a str>,
    pub input_schema: Option<&'a str>,
    pub output_schema: Option<&'a str>,
}

/// Parameters for updating an existing capability.
pub struct UpdateCapabilityParams<'a> {
    pub id: Uuid,
    pub name: &'a str,
    pub description: &'a str,
    pub endpoint_url: &'a str,
    pub auth_type: &'a str,
    pub credential_ref: Option<&'a str>,
    pub input_schema: Option<&'a str>,
    pub output_schema: Option<&'a str>,
}

impl Capability {
    pub fn create(conn: &Connection, p: CreateCapabilityParams<'_>) -> Result<Capability> {
        let id = Uuid::new_v4();
        let now = chrono::Utc::now().to_rfc3339();
        let cap = Capability {
            id,
            tenant_id: p.tenant_id,
            agent_id: p.agent_id,
            name: p.name.to_string(),
            description: p.description.to_string(),
            endpoint_url: p.endpoint_url.to_string(),
            auth_type: p.auth_type.to_string(),
            credential_ref: p.credential_ref.map(|s| s.to_string()),
            input_schema: p.input_schema.map(|s| s.to_string()),
            output_schema: p.output_schema.map(|s| s.to_string()),
            created_at: now.clone(),
            updated_at: now,
        };
        conn.execute(
            "INSERT INTO capabilities (id, tenant_id, agent_id, name, description, endpoint_url,
             auth_type, credential_ref, input_schema, output_schema, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                cap.id.to_string(),
                cap.tenant_id.to_string(),
                cap.agent_id.to_string(),
                cap.name,
                cap.description,
                cap.endpoint_url,
                cap.auth_type,
                cap.credential_ref,
                cap.input_schema,
                cap.output_schema,
                cap.created_at,
                cap.updated_at,
            ],
        )?;
        Ok(cap)
    }

    pub fn get(conn: &Connection, id: Uuid) -> Result<Option<Capability>> {
        let sql = format!("SELECT {} FROM capabilities WHERE id = ?1", SELECT_COLS);
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query_map(params![id.to_string()], row_to_capability)?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    pub fn list_by_agent(conn: &Connection, tenant_id: Uuid, agent_id: Uuid) -> Result<Vec<Capability>> {
        let sql = format!(
            "SELECT {} FROM capabilities WHERE tenant_id = ?1 AND agent_id = ?2 ORDER BY name",
            SELECT_COLS
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(
            params![tenant_id.to_string(), agent_id.to_string()],
            row_to_capability,
        )?;
        let mut caps = Vec::new();
        for row in rows {
            caps.push(row?);
        }
        Ok(caps)
    }

    pub fn update(conn: &Connection, p: UpdateCapabilityParams<'_>) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE capabilities SET name = ?1, description = ?2, endpoint_url = ?3,
             auth_type = ?4, credential_ref = ?5, input_schema = ?6, output_schema = ?7,
             updated_at = ?8 WHERE id = ?9",
            params![
                p.name,
                p.description,
                p.endpoint_url,
                p.auth_type,
                p.credential_ref,
                p.input_schema,
                p.output_schema,
                now,
                p.id.to_string(),
            ],
        )?;
        Ok(())
    }

    pub fn delete(conn: &Connection, id: Uuid) -> Result<()> {
        conn.execute("DELETE FROM capabilities WHERE id = ?1", params![id.to_string()])?;
        Ok(())
    }
}
