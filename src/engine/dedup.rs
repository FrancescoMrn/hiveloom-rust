use std::collections::HashMap;
use std::sync::Mutex;

use crate::store::models::DedupEntry;

pub struct DedupTable {
    seen: Mutex<HashMap<String, chrono::DateTime<chrono::Utc>>>,
}

impl DedupTable {
    pub fn new() -> Self {
        Self {
            seen: Mutex::new(HashMap::new()),
        }
    }

    /// Load recent (non-expired) entries from SQLite on startup.
    pub fn load_from_store(conn: &rusqlite::Connection) -> anyhow::Result<Self> {
        let now = chrono::Utc::now();
        let now_str = now.to_rfc3339();
        let mut stmt = conn.prepare(
            "SELECT delivery_id, received_at FROM dedup_entries WHERE expires_at > ?1",
        )?;
        let rows = stmt.query_map(rusqlite::params![now_str], |row| {
            let delivery_id: String = row.get(0)?;
            let received_at_str: String = row.get(1)?;
            Ok((delivery_id, received_at_str))
        })?;

        let mut map = HashMap::new();
        for row in rows {
            let (delivery_id, received_at_str) = row?;
            if let Ok(ts) = received_at_str.parse::<chrono::DateTime<chrono::Utc>>() {
                map.insert(delivery_id, ts);
            }
        }

        Ok(Self {
            seen: Mutex::new(map),
        })
    }

    /// Check if delivery_id was seen. If not, record it.
    /// Returns `true` if the delivery is NEW (not a duplicate).
    pub fn check_and_record(
        &self,
        conn: &rusqlite::Connection,
        delivery_id: &str,
        tenant_id: &uuid::Uuid,
        surface_type: &str,
    ) -> anyhow::Result<bool> {
        let mut seen = self
            .seen
            .lock()
            .map_err(|e| anyhow::anyhow!("dedup lock poisoned: {}", e))?;

        if seen.contains_key(delivery_id) {
            return Ok(false); // duplicate
        }

        // Try to insert into SQLite (returns true if new)
        let is_new = DedupEntry::check_and_insert(conn, delivery_id, *tenant_id, surface_type)?;

        if is_new {
            seen.insert(delivery_id.to_string(), chrono::Utc::now());
        }

        Ok(is_new)
    }

    /// Remove expired entries (older than 24h) from both in-memory map and SQLite.
    pub fn cleanup(&self, conn: &rusqlite::Connection) -> anyhow::Result<()> {
        let cutoff = chrono::Utc::now() - chrono::Duration::hours(24);

        // Clean in-memory map
        {
            let mut seen = self
                .seen
                .lock()
                .map_err(|e| anyhow::anyhow!("dedup lock poisoned: {}", e))?;
            seen.retain(|_, ts| *ts > cutoff);
        }

        // Clean SQLite
        DedupEntry::cleanup_expired(conn)?;

        Ok(())
    }
}

impl Default for DedupTable {
    fn default() -> Self {
        Self::new()
    }
}
