use anyhow::Result;
use rusqlite::{params, Connection};
use uuid::Uuid;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EventSubscription {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub agent_id: Uuid,
    pub event_type: String,
    pub source_filter: Option<String>,
    pub auth_token_hash: String,
    pub status: String,
    pub created_at: String,
}

const SELECT_COLS: &str =
    "id, tenant_id, agent_id, event_type, source_filter, auth_token_hash, status, created_at";

fn row_to_event_subscription(row: &rusqlite::Row) -> rusqlite::Result<EventSubscription> {
    Ok(EventSubscription {
        id: row.get::<_, String>(0)?.parse().unwrap(),
        tenant_id: row.get::<_, String>(1)?.parse().unwrap(),
        agent_id: row.get::<_, String>(2)?.parse().unwrap(),
        event_type: row.get(3)?,
        source_filter: row.get(4)?,
        auth_token_hash: row.get(5)?,
        status: row.get(6)?,
        created_at: row.get(7)?,
    })
}

impl EventSubscription {
    pub fn create(
        conn: &Connection,
        tenant_id: Uuid,
        agent_id: Uuid,
        event_type: &str,
        source_filter: Option<&str>,
        auth_token_hash: &str,
    ) -> Result<EventSubscription> {
        let id = Uuid::new_v4();
        let now = chrono::Utc::now().to_rfc3339();
        let sub = EventSubscription {
            id,
            tenant_id,
            agent_id,
            event_type: event_type.to_string(),
            source_filter: source_filter.map(|s| s.to_string()),
            auth_token_hash: auth_token_hash.to_string(),
            status: "active".to_string(),
            created_at: now,
        };
        conn.execute(
            "INSERT INTO event_subscriptions (id, tenant_id, agent_id, event_type, source_filter,
             auth_token_hash, status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                sub.id.to_string(),
                sub.tenant_id.to_string(),
                sub.agent_id.to_string(),
                sub.event_type,
                sub.source_filter,
                sub.auth_token_hash,
                sub.status,
                sub.created_at,
            ],
        )?;
        Ok(sub)
    }

    pub fn get(conn: &Connection, id: Uuid) -> Result<Option<EventSubscription>> {
        let sql = format!(
            "SELECT {} FROM event_subscriptions WHERE id = ?1",
            SELECT_COLS
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query_map(params![id.to_string()], row_to_event_subscription)?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    pub fn list_by_agent(
        conn: &Connection,
        tenant_id: Uuid,
        agent_id: Uuid,
    ) -> Result<Vec<EventSubscription>> {
        let sql = format!(
            "SELECT {} FROM event_subscriptions WHERE tenant_id = ?1 AND agent_id = ?2 ORDER BY created_at",
            SELECT_COLS
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(
            params![tenant_id.to_string(), agent_id.to_string()],
            row_to_event_subscription,
        )?;
        let mut subs = Vec::new();
        for row in rows {
            subs.push(row?);
        }
        Ok(subs)
    }

    pub fn list_by_event_type(
        conn: &Connection,
        tenant_id: Uuid,
        event_type: &str,
    ) -> Result<Vec<EventSubscription>> {
        let sql = format!(
            "SELECT {} FROM event_subscriptions WHERE tenant_id = ?1 AND event_type = ?2 AND status = 'active'",
            SELECT_COLS
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(
            params![tenant_id.to_string(), event_type],
            row_to_event_subscription,
        )?;
        let mut subs = Vec::new();
        for row in rows {
            subs.push(row?);
        }
        Ok(subs)
    }

    /// Validate an auth token hash against a subscription's stored hash.
    /// Returns true if match.
    pub fn validate_auth_token(conn: &Connection, id: Uuid, token_hash: &str) -> Result<bool> {
        let sub = Self::get(conn, id)?;
        match sub {
            Some(s) => Ok(s.auth_token_hash == token_hash),
            None => Ok(false),
        }
    }

    pub fn disable(conn: &Connection, id: Uuid) -> Result<()> {
        conn.execute(
            "UPDATE event_subscriptions SET status = 'disabled' WHERE id = ?1",
            params![id.to_string()],
        )?;
        Ok(())
    }

    pub fn enable(conn: &Connection, id: Uuid) -> Result<()> {
        conn.execute(
            "UPDATE event_subscriptions SET status = 'active' WHERE id = ?1",
            params![id.to_string()],
        )?;
        Ok(())
    }

    pub fn delete(conn: &Connection, id: Uuid) -> Result<()> {
        conn.execute(
            "DELETE FROM event_subscriptions WHERE id = ?1",
            params![id.to_string()],
        )?;
        Ok(())
    }
}
