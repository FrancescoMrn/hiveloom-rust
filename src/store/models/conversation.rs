use anyhow::Result;
use rusqlite::{params, Connection};
use uuid::Uuid;

// ── Conversation ──────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Conversation {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub agent_id: Uuid,
    pub surface_type: String,
    pub surface_ref: String,
    pub user_identity: String,
    pub thread_ref: Option<String>,
    pub status: String,
    pub workflow_state: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub concluded_at: Option<String>,
    pub abandoned_at: Option<String>,
}

const CONV_COLS: &str =
    "id, tenant_id, agent_id, surface_type, surface_ref, user_identity, thread_ref, status, \
     workflow_state, created_at, updated_at, concluded_at, abandoned_at";

fn row_to_conversation(row: &rusqlite::Row) -> rusqlite::Result<Conversation> {
    Ok(Conversation {
        id: row.get::<_, String>(0)?.parse().unwrap(),
        tenant_id: row.get::<_, String>(1)?.parse().unwrap(),
        agent_id: row.get::<_, String>(2)?.parse().unwrap(),
        surface_type: row.get(3)?,
        surface_ref: row.get(4)?,
        user_identity: row.get(5)?,
        thread_ref: row.get(6)?,
        status: row.get(7)?,
        workflow_state: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
        concluded_at: row.get(11)?,
        abandoned_at: row.get(12)?,
    })
}

impl Conversation {
    pub fn create(
        conn: &Connection,
        tenant_id: Uuid,
        agent_id: Uuid,
        surface_type: &str,
        surface_ref: &str,
        user_identity: &str,
        thread_ref: Option<&str>,
    ) -> Result<Conversation> {
        let id = Uuid::new_v4();
        let now = chrono::Utc::now().to_rfc3339();
        let conv = Conversation {
            id,
            tenant_id,
            agent_id,
            surface_type: surface_type.to_string(),
            surface_ref: surface_ref.to_string(),
            user_identity: user_identity.to_string(),
            thread_ref: thread_ref.map(|s| s.to_string()),
            status: "active".to_string(),
            workflow_state: None,
            created_at: now.clone(),
            updated_at: now,
            concluded_at: None,
            abandoned_at: None,
        };
        conn.execute(
            "INSERT INTO conversations (id, tenant_id, agent_id, surface_type, surface_ref,
             user_identity, thread_ref, status, workflow_state, created_at, updated_at,
             concluded_at, abandoned_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                conv.id.to_string(),
                conv.tenant_id.to_string(),
                conv.agent_id.to_string(),
                conv.surface_type,
                conv.surface_ref,
                conv.user_identity,
                conv.thread_ref,
                conv.status,
                conv.workflow_state,
                conv.created_at,
                conv.updated_at,
                conv.concluded_at,
                conv.abandoned_at,
            ],
        )?;
        Ok(conv)
    }

    pub fn get(conn: &Connection, id: Uuid) -> Result<Option<Conversation>> {
        let sql = format!("SELECT {} FROM conversations WHERE id = ?1", CONV_COLS);
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query_map(params![id.to_string()], row_to_conversation)?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    pub fn get_active_by_surface(
        conn: &Connection,
        tenant_id: Uuid,
        surface_ref: &str,
    ) -> Result<Option<Conversation>> {
        let sql = format!(
            "SELECT {} FROM conversations
             WHERE tenant_id = ?1 AND surface_ref = ?2 AND status = 'active'
             ORDER BY created_at DESC LIMIT 1",
            CONV_COLS
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query_map(
            params![tenant_id.to_string(), surface_ref],
            row_to_conversation,
        )?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    pub fn update_status(conn: &Connection, id: Uuid, status: &str) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        let (concluded, abandoned) = match status {
            "concluded" => (Some(now.clone()), None),
            "abandoned" => (None, Some(now.clone())),
            _ => (None, None),
        };
        conn.execute(
            "UPDATE conversations SET status = ?1, updated_at = ?2, concluded_at = COALESCE(?3, concluded_at),
             abandoned_at = COALESCE(?4, abandoned_at) WHERE id = ?5",
            params![status, now, concluded, abandoned, id.to_string()],
        )?;
        Ok(())
    }

    pub fn set_workflow_state(conn: &Connection, id: Uuid, workflow_state: &str) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE conversations SET workflow_state = ?1, updated_at = ?2 WHERE id = ?3",
            params![workflow_state, now, id.to_string()],
        )?;
        Ok(())
    }
}

// ── ConversationTurn ──────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ConversationTurn {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub conversation_id: Uuid,
    pub turn_index: i64,
    pub role: String,
    pub content: String,
    pub token_count: i64,
    pub created_at: String,
}

impl ConversationTurn {
    pub fn append(
        conn: &Connection,
        conversation_id: Uuid,
        tenant_id: Uuid,
        role: &str,
        content: &str,
        token_count: i64,
    ) -> Result<ConversationTurn> {
        // Get next turn_index
        let next_index: i64 = conn.query_row(
            "SELECT COALESCE(MAX(turn_index), -1) + 1 FROM conversation_turns WHERE conversation_id = ?1",
            params![conversation_id.to_string()],
            |row| row.get(0),
        )?;

        let id = Uuid::new_v4();
        let now = chrono::Utc::now().to_rfc3339();
        let turn = ConversationTurn {
            id,
            tenant_id,
            conversation_id,
            turn_index: next_index,
            role: role.to_string(),
            content: content.to_string(),
            token_count,
            created_at: now,
        };
        conn.execute(
            "INSERT INTO conversation_turns (id, tenant_id, conversation_id, turn_index, role,
             content, token_count, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                turn.id.to_string(),
                turn.tenant_id.to_string(),
                turn.conversation_id.to_string(),
                turn.turn_index,
                turn.role,
                turn.content,
                turn.token_count,
                turn.created_at,
            ],
        )?;
        Ok(turn)
    }

    pub fn list_by_conversation(conn: &Connection, conversation_id: Uuid) -> Result<Vec<ConversationTurn>> {
        let mut stmt = conn.prepare(
            "SELECT id, tenant_id, conversation_id, turn_index, role, content, token_count, created_at
             FROM conversation_turns WHERE conversation_id = ?1 ORDER BY turn_index",
        )?;
        let rows = stmt.query_map(params![conversation_id.to_string()], |row| {
            Ok(ConversationTurn {
                id: row.get::<_, String>(0)?.parse().unwrap(),
                tenant_id: row.get::<_, String>(1)?.parse().unwrap(),
                conversation_id: row.get::<_, String>(2)?.parse().unwrap(),
                turn_index: row.get(3)?,
                role: row.get(4)?,
                content: row.get(5)?,
                token_count: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?;
        let mut turns = Vec::new();
        for row in rows {
            turns.push(row?);
        }
        Ok(turns)
    }
}
