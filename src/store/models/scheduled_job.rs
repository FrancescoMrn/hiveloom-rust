use anyhow::Result;
use rusqlite::{params, Connection};
use uuid::Uuid;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ScheduledJob {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub agent_id: Uuid,
    pub cron_expression: Option<String>,
    pub one_time_at: Option<String>,
    pub timezone: String,
    pub initial_context: String,
    pub status: String,
    pub last_fired_at: Option<String>,
    pub next_fire_at: Option<String>,
    pub created_at: String,
}

const SELECT_COLS: &str =
    "id, tenant_id, agent_id, cron_expression, one_time_at, timezone, initial_context, \
     status, last_fired_at, next_fire_at, created_at";

fn row_to_scheduled_job(row: &rusqlite::Row) -> rusqlite::Result<ScheduledJob> {
    Ok(ScheduledJob {
        id: row.get::<_, String>(0)?.parse().unwrap(),
        tenant_id: row.get::<_, String>(1)?.parse().unwrap(),
        agent_id: row.get::<_, String>(2)?.parse().unwrap(),
        cron_expression: row.get(3)?,
        one_time_at: row.get(4)?,
        timezone: row.get(5)?,
        initial_context: row.get(6)?,
        status: row.get(7)?,
        last_fired_at: row.get(8)?,
        next_fire_at: row.get(9)?,
        created_at: row.get(10)?,
    })
}

impl ScheduledJob {
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        conn: &Connection,
        tenant_id: Uuid,
        agent_id: Uuid,
        cron_expression: Option<&str>,
        one_time_at: Option<&str>,
        timezone: &str,
        initial_context: &str,
        next_fire_at: Option<&str>,
    ) -> Result<ScheduledJob> {
        let id = Uuid::new_v4();
        let now = chrono::Utc::now().to_rfc3339();
        let job = ScheduledJob {
            id,
            tenant_id,
            agent_id,
            cron_expression: cron_expression.map(|s| s.to_string()),
            one_time_at: one_time_at.map(|s| s.to_string()),
            timezone: timezone.to_string(),
            initial_context: initial_context.to_string(),
            status: "active".to_string(),
            last_fired_at: None,
            next_fire_at: next_fire_at.map(|s| s.to_string()),
            created_at: now,
        };
        conn.execute(
            "INSERT INTO scheduled_jobs (id, tenant_id, agent_id, cron_expression, one_time_at,
             timezone, initial_context, status, last_fired_at, next_fire_at, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                job.id.to_string(),
                job.tenant_id.to_string(),
                job.agent_id.to_string(),
                job.cron_expression,
                job.one_time_at,
                job.timezone,
                job.initial_context,
                job.status,
                job.last_fired_at,
                job.next_fire_at,
                job.created_at,
            ],
        )?;
        Ok(job)
    }

    pub fn get(conn: &Connection, id: Uuid) -> Result<Option<ScheduledJob>> {
        let sql = format!("SELECT {} FROM scheduled_jobs WHERE id = ?1", SELECT_COLS);
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query_map(params![id.to_string()], row_to_scheduled_job)?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    pub fn list_by_agent(
        conn: &Connection,
        tenant_id: Uuid,
        agent_id: Uuid,
    ) -> Result<Vec<ScheduledJob>> {
        let sql = format!(
            "SELECT {} FROM scheduled_jobs WHERE tenant_id = ?1 AND agent_id = ?2 ORDER BY created_at",
            SELECT_COLS
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(
            params![tenant_id.to_string(), agent_id.to_string()],
            row_to_scheduled_job,
        )?;
        let mut jobs = Vec::new();
        for row in rows {
            jobs.push(row?);
        }
        Ok(jobs)
    }

    /// Return all active jobs whose next_fire_at <= now.
    pub fn list_due(conn: &Connection, now: &str) -> Result<Vec<ScheduledJob>> {
        let sql = format!(
            "SELECT {} FROM scheduled_jobs WHERE status = 'active' AND next_fire_at IS NOT NULL AND next_fire_at <= ?1",
            SELECT_COLS
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![now], row_to_scheduled_job)?;
        let mut jobs = Vec::new();
        for row in rows {
            jobs.push(row?);
        }
        Ok(jobs)
    }

    /// Return all active jobs across all tenants (for scheduler startup scan).
    pub fn list_active(conn: &Connection) -> Result<Vec<ScheduledJob>> {
        let sql = format!(
            "SELECT {} FROM scheduled_jobs WHERE status = 'active'",
            SELECT_COLS
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], row_to_scheduled_job)?;
        let mut jobs = Vec::new();
        for row in rows {
            jobs.push(row?);
        }
        Ok(jobs)
    }

    pub fn update_last_fired(conn: &Connection, id: Uuid, now: &str) -> Result<()> {
        conn.execute(
            "UPDATE scheduled_jobs SET last_fired_at = ?1 WHERE id = ?2",
            params![now, id.to_string()],
        )?;
        Ok(())
    }

    pub fn update_next_fire(conn: &Connection, id: Uuid, next_fire_at: Option<&str>) -> Result<()> {
        conn.execute(
            "UPDATE scheduled_jobs SET next_fire_at = ?1 WHERE id = ?2",
            params![next_fire_at, id.to_string()],
        )?;
        Ok(())
    }

    pub fn pause(conn: &Connection, id: Uuid) -> Result<()> {
        conn.execute(
            "UPDATE scheduled_jobs SET status = 'paused' WHERE id = ?1",
            params![id.to_string()],
        )?;
        Ok(())
    }

    pub fn resume(conn: &Connection, id: Uuid) -> Result<()> {
        conn.execute(
            "UPDATE scheduled_jobs SET status = 'active' WHERE id = ?1",
            params![id.to_string()],
        )?;
        Ok(())
    }

    pub fn disable(conn: &Connection, id: Uuid) -> Result<()> {
        conn.execute(
            "UPDATE scheduled_jobs SET status = 'disabled' WHERE id = ?1",
            params![id.to_string()],
        )?;
        Ok(())
    }

    pub fn delete(conn: &Connection, id: Uuid) -> Result<()> {
        conn.execute(
            "DELETE FROM scheduled_jobs WHERE id = ?1",
            params![id.to_string()],
        )?;
        Ok(())
    }
}
