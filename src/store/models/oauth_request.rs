use anyhow::Result;
use rusqlite::{params, Connection};
use uuid::Uuid;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OAuthAuthorizationRequest {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub user_identity: String,
    pub provider: String,
    pub state_token: String,
    pub requested_scopes: Option<String>,
    pub paused_run_ref: Option<String>,
    pub surface_type: Option<String>,
    pub expires_at: String,
    pub completed_at: Option<String>,
    pub created_at: String,
}

const SELECT_COLS: &str = "id, tenant_id, user_identity, provider, state_token, requested_scopes, \
     paused_run_ref, surface_type, expires_at, completed_at, created_at";

fn row_to_oauth_request(row: &rusqlite::Row) -> rusqlite::Result<OAuthAuthorizationRequest> {
    Ok(OAuthAuthorizationRequest {
        id: row.get::<_, String>(0)?.parse().unwrap(),
        tenant_id: row.get::<_, String>(1)?.parse().unwrap(),
        user_identity: row.get(2)?,
        provider: row.get(3)?,
        state_token: row.get(4)?,
        requested_scopes: row.get(5)?,
        paused_run_ref: row.get(6)?,
        surface_type: row.get(7)?,
        expires_at: row.get(8)?,
        completed_at: row.get(9)?,
        created_at: row.get(10)?,
    })
}

impl OAuthAuthorizationRequest {
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        conn: &Connection,
        tenant_id: Uuid,
        user_identity: &str,
        provider: &str,
        state_token: &str,
        requested_scopes: Option<&str>,
        paused_run_ref: Option<&str>,
        surface_type: Option<&str>,
        expires_at: &str,
    ) -> Result<OAuthAuthorizationRequest> {
        let id = Uuid::new_v4();
        let now = chrono::Utc::now().to_rfc3339();
        let entry = OAuthAuthorizationRequest {
            id,
            tenant_id,
            user_identity: user_identity.to_string(),
            provider: provider.to_string(),
            state_token: state_token.to_string(),
            requested_scopes: requested_scopes.map(|s| s.to_string()),
            paused_run_ref: paused_run_ref.map(|s| s.to_string()),
            surface_type: surface_type.map(|s| s.to_string()),
            expires_at: expires_at.to_string(),
            completed_at: None,
            created_at: now,
        };
        conn.execute(
            "INSERT INTO oauth_authorization_requests
             (id, tenant_id, user_identity, provider, state_token, requested_scopes,
              paused_run_ref, surface_type, expires_at, completed_at, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                entry.id.to_string(),
                entry.tenant_id.to_string(),
                entry.user_identity,
                entry.provider,
                entry.state_token,
                entry.requested_scopes,
                entry.paused_run_ref,
                entry.surface_type,
                entry.expires_at,
                entry.completed_at,
                entry.created_at,
            ],
        )?;
        Ok(entry)
    }

    pub fn get(conn: &Connection, id: Uuid) -> Result<Option<OAuthAuthorizationRequest>> {
        let sql = format!(
            "SELECT {} FROM oauth_authorization_requests WHERE id = ?1",
            SELECT_COLS
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query_map(params![id.to_string()], row_to_oauth_request)?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    pub fn get_by_state_token(
        conn: &Connection,
        state_token: &str,
    ) -> Result<Option<OAuthAuthorizationRequest>> {
        let sql = format!(
            "SELECT {} FROM oauth_authorization_requests WHERE state_token = ?1",
            SELECT_COLS
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query_map(params![state_token], row_to_oauth_request)?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    pub fn mark_completed(conn: &Connection, id: Uuid) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE oauth_authorization_requests SET completed_at = ?1 WHERE id = ?2",
            params![now, id.to_string()],
        )?;
        Ok(())
    }

    pub fn cleanup_expired(conn: &Connection) -> Result<usize> {
        let now = chrono::Utc::now().to_rfc3339();
        let count = conn.execute(
            "DELETE FROM oauth_authorization_requests
             WHERE completed_at IS NULL AND expires_at < ?1",
            params![now],
        )?;
        Ok(count)
    }
}
