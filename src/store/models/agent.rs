use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use uuid::Uuid;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Agent {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub name: String,
    pub system_prompt: String,
    pub model_id: String,
    pub scope_mode: String,
    pub default_scope_policy: String,
    pub scope_coerce_policy: String,
    pub reflection_enabled: bool,
    pub reflection_cron: Option<String>,
    pub status: String,
    pub version: i64,
    pub is_current: bool,
    pub parent_version_id: Option<String>,
    pub created_at: String,
}

fn row_to_agent(row: &rusqlite::Row) -> rusqlite::Result<Agent> {
    Ok(Agent {
        id: row.get::<_, String>(0)?.parse().unwrap(),
        tenant_id: row.get::<_, String>(1)?.parse().unwrap(),
        name: row.get(2)?,
        system_prompt: row.get(3)?,
        model_id: row.get(4)?,
        scope_mode: row.get(5)?,
        default_scope_policy: row.get(6)?,
        scope_coerce_policy: row.get(7)?,
        reflection_enabled: row.get::<_, i64>(8)? != 0,
        reflection_cron: row.get(9)?,
        status: row.get(10)?,
        version: row.get(11)?,
        is_current: row.get::<_, i64>(12)? != 0,
        parent_version_id: row.get(13)?,
        created_at: row.get(14)?,
    })
}

const SELECT_COLS: &str =
    "id, tenant_id, name, system_prompt, model_id, scope_mode, default_scope_policy, \
     scope_coerce_policy, reflection_enabled, reflection_cron, status, version, is_current, \
     parent_version_id, created_at";

impl Agent {
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        conn: &Connection,
        tenant_id: Uuid,
        name: &str,
        system_prompt: &str,
        model_id: &str,
        scope_mode: &str,
        default_scope_policy: &str,
        scope_coerce_policy: &str,
        reflection_enabled: bool,
        reflection_cron: Option<&str>,
    ) -> Result<Agent> {
        let id = Uuid::new_v4();
        let now = chrono::Utc::now().to_rfc3339();
        let agent = Agent {
            id,
            tenant_id,
            name: name.to_string(),
            system_prompt: system_prompt.to_string(),
            model_id: model_id.to_string(),
            scope_mode: scope_mode.to_string(),
            default_scope_policy: default_scope_policy.to_string(),
            scope_coerce_policy: scope_coerce_policy.to_string(),
            reflection_enabled,
            reflection_cron: reflection_cron.map(|s| s.to_string()),
            status: "active".to_string(),
            version: 1,
            is_current: true,
            parent_version_id: None,
            created_at: now,
        };
        conn.execute(
            "INSERT INTO agents (id, tenant_id, name, system_prompt, model_id, scope_mode,
             default_scope_policy, scope_coerce_policy, reflection_enabled, reflection_cron,
             status, version, is_current, parent_version_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                agent.id.to_string(),
                agent.tenant_id.to_string(),
                agent.name,
                agent.system_prompt,
                agent.model_id,
                agent.scope_mode,
                agent.default_scope_policy,
                agent.scope_coerce_policy,
                agent.reflection_enabled as i64,
                agent.reflection_cron,
                agent.status,
                agent.version,
                agent.is_current as i64,
                agent.parent_version_id,
                agent.created_at,
            ],
        )?;
        Ok(agent)
    }

    pub fn get_current(conn: &Connection, tenant_id: Uuid, id: Uuid) -> Result<Option<Agent>> {
        let sql = format!(
            "SELECT {} FROM agents WHERE id = ?1 AND tenant_id = ?2 AND is_current = 1",
            SELECT_COLS
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut rows =
            stmt.query_map(params![id.to_string(), tenant_id.to_string()], row_to_agent)?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    pub fn list_current(conn: &Connection, tenant_id: Uuid) -> Result<Vec<Agent>> {
        let sql = format!(
            "SELECT {} FROM agents WHERE tenant_id = ?1 AND is_current = 1 ORDER BY name",
            SELECT_COLS
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![tenant_id.to_string()], row_to_agent)?;
        let mut agents = Vec::new();
        for row in rows {
            agents.push(row?);
        }
        Ok(agents)
    }

    pub fn get_version(conn: &Connection, id: Uuid, version: i64) -> Result<Option<Agent>> {
        let sql = format!(
            "SELECT {} FROM agents WHERE id = ?1 AND version = ?2",
            SELECT_COLS
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query_map(params![id.to_string(), version], row_to_agent)?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    pub fn list_versions(conn: &Connection, id: Uuid) -> Result<Vec<Agent>> {
        let sql = format!(
            "SELECT {} FROM agents WHERE id = ?1 ORDER BY version",
            SELECT_COLS
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![id.to_string()], row_to_agent)?;
        let mut agents = Vec::new();
        for row in rows {
            agents.push(row?);
        }
        Ok(agents)
    }

    pub fn create_new_version(conn: &Connection, agent: &Agent) -> Result<Agent> {
        let tx = conn.unchecked_transaction()?;

        // Flip old current to not-current
        tx.execute(
            "UPDATE agents SET is_current = 0 WHERE id = ?1 AND is_current = 1",
            params![agent.id.to_string()],
        )?;

        let now = chrono::Utc::now().to_rfc3339();
        let max_version: i64 = tx.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM agents WHERE id = ?1",
            params![agent.id.to_string()],
            |row| row.get(0),
        )?;
        let new_version = max_version + 1;
        let new_agent = Agent {
            id: agent.id,
            tenant_id: agent.tenant_id,
            name: agent.name.clone(),
            system_prompt: agent.system_prompt.clone(),
            model_id: agent.model_id.clone(),
            scope_mode: agent.scope_mode.clone(),
            default_scope_policy: agent.default_scope_policy.clone(),
            scope_coerce_policy: agent.scope_coerce_policy.clone(),
            reflection_enabled: agent.reflection_enabled,
            reflection_cron: agent.reflection_cron.clone(),
            status: agent.status.clone(),
            version: new_version,
            is_current: true,
            parent_version_id: Some(format!("{}@{}", agent.id, agent.version)),
            created_at: now,
        };
        tx.execute(
            "INSERT INTO agents (id, tenant_id, name, system_prompt, model_id, scope_mode,
             default_scope_policy, scope_coerce_policy, reflection_enabled, reflection_cron,
             status, version, is_current, parent_version_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                new_agent.id.to_string(),
                new_agent.tenant_id.to_string(),
                new_agent.name,
                new_agent.system_prompt,
                new_agent.model_id,
                new_agent.scope_mode,
                new_agent.default_scope_policy,
                new_agent.scope_coerce_policy,
                new_agent.reflection_enabled as i64,
                new_agent.reflection_cron,
                new_agent.status,
                new_agent.version,
                new_agent.is_current as i64,
                new_agent.parent_version_id,
                new_agent.created_at,
            ],
        )?;
        tx.commit()?;
        Ok(new_agent)
    }

    pub fn rollback(conn: &Connection, id: Uuid, to_version: i64) -> Result<Agent> {
        let old = Self::get_version(conn, id, to_version)?.context("target version not found")?;
        Self::create_new_version(conn, &old)
    }

    pub fn delete(conn: &Connection, id: Uuid) -> Result<()> {
        conn.execute(
            "UPDATE agents SET status = 'disabled', is_current = 0 WHERE id = ?1 AND is_current = 1",
            params![id.to_string()],
        )?;
        Ok(())
    }

    pub fn update(conn: &Connection, agent: &Agent) -> Result<Agent> {
        // Create a new version with updated fields
        Self::create_new_version(conn, agent)
    }
}
