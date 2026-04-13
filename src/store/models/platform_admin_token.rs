use anyhow::Result;
use rusqlite::{params, Connection};
use uuid::Uuid;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PlatformAdminToken {
    pub id: Uuid,
    pub token_hash: String,
    pub scope: String,
    pub created_at: String,
    pub expires_at: Option<String>,
    pub revoked_at: Option<String>,
}

impl PlatformAdminToken {
    pub fn create(
        conn: &Connection,
        token_hash: &str,
        scope: &str,
        expires_at: Option<&str>,
    ) -> Result<PlatformAdminToken> {
        let id = Uuid::new_v4();
        let now = chrono::Utc::now().to_rfc3339();
        let token = PlatformAdminToken {
            id,
            token_hash: token_hash.to_string(),
            scope: scope.to_string(),
            created_at: now,
            expires_at: expires_at.map(|s| s.to_string()),
            revoked_at: None,
        };
        conn.execute(
            "INSERT INTO platform_admin_tokens (id, token_hash, scope, created_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                token.id.to_string(),
                token.token_hash,
                token.scope,
                token.created_at,
                token.expires_at,
            ],
        )?;
        Ok(token)
    }

    pub fn validate(conn: &Connection, token_hash: &str) -> Result<Option<PlatformAdminToken>> {
        let now = chrono::Utc::now().to_rfc3339();
        let mut stmt = conn.prepare(
            "SELECT id, token_hash, scope, created_at, expires_at, revoked_at
             FROM platform_admin_tokens
             WHERE token_hash = ?1
               AND revoked_at IS NULL
               AND (expires_at IS NULL OR expires_at > ?2)",
        )?;
        let mut rows = stmt.query_map(params![token_hash, now], |row| {
            Ok(PlatformAdminToken {
                id: row.get::<_, String>(0)?.parse().unwrap(),
                token_hash: row.get(1)?,
                scope: row.get(2)?,
                created_at: row.get(3)?,
                expires_at: row.get(4)?,
                revoked_at: row.get(5)?,
            })
        })?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    pub fn list(conn: &Connection) -> Result<Vec<PlatformAdminToken>> {
        let mut stmt = conn.prepare(
            "SELECT id, token_hash, scope, created_at, expires_at, revoked_at
             FROM platform_admin_tokens ORDER BY created_at",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(PlatformAdminToken {
                id: row.get::<_, String>(0)?.parse().unwrap(),
                token_hash: row.get(1)?,
                scope: row.get(2)?,
                created_at: row.get(3)?,
                expires_at: row.get(4)?,
                revoked_at: row.get(5)?,
            })
        })?;
        let mut tokens = Vec::new();
        for row in rows {
            tokens.push(row?);
        }
        Ok(tokens)
    }

    pub fn revoke(conn: &Connection, id: Uuid) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE platform_admin_tokens SET revoked_at = ?1 WHERE id = ?2",
            params![now, id.to_string()],
        )?;
        Ok(())
    }
}
