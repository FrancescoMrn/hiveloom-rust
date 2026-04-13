use anyhow::Result;
use rusqlite::{params, Connection};
use uuid::Uuid;

/// An OAuth client registered via dynamic client registration (POST /oauth/register).
/// Stored in the **platform store** (pre-tenant).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct McpOAuthClient {
    pub id: Uuid,
    pub client_id: String,
    pub client_secret_hash: String,
    pub client_name: Option<String>,
    pub redirect_uris: String, // JSON array
    pub grant_types: String,   // JSON array
    pub token_endpoint_auth_method: String,
    pub created_at: String,
}

const SELECT_COLS: &str =
    "id, client_id, client_secret_hash, client_name, redirect_uris, grant_types, \
     token_endpoint_auth_method, created_at";

fn row_to_oauth_client(row: &rusqlite::Row) -> rusqlite::Result<McpOAuthClient> {
    Ok(McpOAuthClient {
        id: row.get::<_, String>(0)?.parse().unwrap(),
        client_id: row.get(1)?,
        client_secret_hash: row.get(2)?,
        client_name: row.get(3)?,
        redirect_uris: row.get(4)?,
        grant_types: row.get(5)?,
        token_endpoint_auth_method: row.get(6)?,
        created_at: row.get(7)?,
    })
}

impl McpOAuthClient {
    pub fn create(
        conn: &Connection,
        client_id: &str,
        client_secret_hash: &str,
        client_name: Option<&str>,
        redirect_uris: &str,
        grant_types: &str,
        token_endpoint_auth_method: &str,
    ) -> Result<McpOAuthClient> {
        let id = Uuid::new_v4();
        let now = chrono::Utc::now().to_rfc3339();
        let entry = McpOAuthClient {
            id,
            client_id: client_id.to_string(),
            client_secret_hash: client_secret_hash.to_string(),
            client_name: client_name.map(|s| s.to_string()),
            redirect_uris: redirect_uris.to_string(),
            grant_types: grant_types.to_string(),
            token_endpoint_auth_method: token_endpoint_auth_method.to_string(),
            created_at: now,
        };
        conn.execute(
            "INSERT INTO mcp_oauth_clients
             (id, client_id, client_secret_hash, client_name, redirect_uris,
              grant_types, token_endpoint_auth_method, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                entry.id.to_string(),
                entry.client_id,
                entry.client_secret_hash,
                entry.client_name,
                entry.redirect_uris,
                entry.grant_types,
                entry.token_endpoint_auth_method,
                entry.created_at,
            ],
        )?;
        Ok(entry)
    }

    pub fn get_by_client_id(conn: &Connection, client_id: &str) -> Result<Option<McpOAuthClient>> {
        let sql = format!(
            "SELECT {} FROM mcp_oauth_clients WHERE client_id = ?1",
            SELECT_COLS
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query_map(params![client_id], row_to_oauth_client)?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }
}
