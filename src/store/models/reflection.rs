use anyhow::Result;
use rusqlite::{params, Connection};
use uuid::Uuid;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ReflectionReport {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub agent_id: Uuid,
    pub trigger: String,
    pub window_start: String,
    pub window_end: String,
    pub skill_suggestions: String,
    pub memory_suggestions: String,
    pub created_at: String,
}

const SELECT_COLS: &str =
    "id, tenant_id, agent_id, trigger, window_start, window_end, \
     skill_suggestions, memory_suggestions, created_at";

fn row_to_report(row: &rusqlite::Row) -> rusqlite::Result<ReflectionReport> {
    Ok(ReflectionReport {
        id: row.get::<_, String>(0)?.parse().unwrap(),
        tenant_id: row.get::<_, String>(1)?.parse().unwrap(),
        agent_id: row.get::<_, String>(2)?.parse().unwrap(),
        trigger: row.get(3)?,
        window_start: row.get(4)?,
        window_end: row.get(5)?,
        skill_suggestions: row.get(6)?,
        memory_suggestions: row.get(7)?,
        created_at: row.get(8)?,
    })
}

impl ReflectionReport {
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        conn: &Connection,
        tenant_id: Uuid,
        agent_id: Uuid,
        trigger: &str,
        window_start: &str,
        window_end: &str,
        skill_suggestions: &str,
        memory_suggestions: &str,
    ) -> Result<ReflectionReport> {
        let id = Uuid::new_v4();
        let now = chrono::Utc::now().to_rfc3339();
        let report = ReflectionReport {
            id,
            tenant_id,
            agent_id,
            trigger: trigger.to_string(),
            window_start: window_start.to_string(),
            window_end: window_end.to_string(),
            skill_suggestions: skill_suggestions.to_string(),
            memory_suggestions: memory_suggestions.to_string(),
            created_at: now,
        };
        conn.execute(
            "INSERT INTO reflection_reports (id, tenant_id, agent_id, trigger, window_start,
             window_end, skill_suggestions, memory_suggestions, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                report.id.to_string(),
                report.tenant_id.to_string(),
                report.agent_id.to_string(),
                report.trigger,
                report.window_start,
                report.window_end,
                report.skill_suggestions,
                report.memory_suggestions,
                report.created_at,
            ],
        )?;
        Ok(report)
    }

    pub fn get(conn: &Connection, id: Uuid) -> Result<Option<ReflectionReport>> {
        let sql = format!("SELECT {} FROM reflection_reports WHERE id = ?1", SELECT_COLS);
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query_map(params![id.to_string()], row_to_report)?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    pub fn list_by_agent(
        conn: &Connection,
        tenant_id: Uuid,
        agent_id: Uuid,
    ) -> Result<Vec<ReflectionReport>> {
        let sql = format!(
            "SELECT {} FROM reflection_reports WHERE tenant_id = ?1 AND agent_id = ?2 ORDER BY created_at DESC",
            SELECT_COLS
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(
            params![tenant_id.to_string(), agent_id.to_string()],
            row_to_report,
        )?;
        let mut reports = Vec::new();
        for row in rows {
            reports.push(row?);
        }
        Ok(reports)
    }

    pub fn delete(conn: &Connection, id: Uuid) -> Result<()> {
        conn.execute(
            "DELETE FROM reflection_reports WHERE id = ?1",
            params![id.to_string()],
        )?;
        Ok(())
    }
}
