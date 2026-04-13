use anyhow::Result;
use rusqlite::{params, Connection};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DedupEntry {
    pub delivery_id: String,
    pub tenant_id: uuid::Uuid,
    pub surface_type: String,
    pub received_at: String,
    pub expires_at: String,
}

impl DedupEntry {
    /// Check if a delivery_id already exists; if not, insert it.
    /// Returns `true` if the entry is new (inserted), `false` if it is a duplicate.
    pub fn check_and_insert(
        conn: &Connection,
        delivery_id: &str,
        tenant_id: uuid::Uuid,
        surface_type: &str,
    ) -> Result<bool> {
        let now = chrono::Utc::now();
        let received_at = now.to_rfc3339();
        // Default TTL: 24 hours
        let expires_at = (now + chrono::Duration::hours(24)).to_rfc3339();

        let rows_inserted = conn.execute(
            "INSERT OR IGNORE INTO dedup_entries (delivery_id, tenant_id, surface_type, received_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                delivery_id,
                tenant_id.to_string(),
                surface_type,
                received_at,
                expires_at,
            ],
        )?;
        Ok(rows_inserted > 0)
    }

    /// Remove all expired dedup entries.
    pub fn cleanup_expired(conn: &Connection) -> Result<u64> {
        let now = chrono::Utc::now().to_rfc3339();
        let deleted = conn.execute(
            "DELETE FROM dedup_entries WHERE expires_at <= ?1",
            params![now],
        )?;
        Ok(deleted as u64)
    }
}
