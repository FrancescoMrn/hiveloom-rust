use anyhow::Result;
use rusqlite::{params, Connection};
use uuid::Uuid;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CredentialVaultEntry {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub agent_id: Option<Uuid>,
    pub name: String,
    pub kind: String,
    #[serde(with = "serde_bytes_base64")]
    pub encrypted_value: Vec<u8>,
    pub provider: Option<String>,
    pub user_identity: Option<String>,
    pub granted_scopes: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub rotated_at: Option<String>,
}

/// Serde helper to serialize Vec<u8> as base64 in JSON.
mod serde_bytes_base64 {
    use base64::Engine;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &[u8], s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&base64::engine::general_purpose::STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        let s = String::deserialize(d)?;
        base64::engine::general_purpose::STANDARD
            .decode(&s)
            .map_err(serde::de::Error::custom)
    }
}

fn row_to_credential(row: &rusqlite::Row) -> rusqlite::Result<CredentialVaultEntry> {
    Ok(CredentialVaultEntry {
        id: row.get::<_, String>(0)?.parse().unwrap(),
        tenant_id: row.get::<_, String>(1)?.parse().unwrap(),
        agent_id: row.get::<_, Option<String>>(2)?.map(|s| s.parse().unwrap()),
        name: row.get(3)?,
        kind: row.get(4)?,
        encrypted_value: row.get(5)?,
        provider: row.get(6)?,
        user_identity: row.get(7)?,
        granted_scopes: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
        rotated_at: row.get(11)?,
    })
}

const SELECT_COLS: &str =
    "id, tenant_id, agent_id, name, kind, encrypted_value, provider, user_identity, \
     granted_scopes, created_at, updated_at, rotated_at";

impl CredentialVaultEntry {
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        conn: &Connection,
        tenant_id: Uuid,
        agent_id: Option<Uuid>,
        name: &str,
        kind: &str,
        encrypted_value: &[u8],
        provider: Option<&str>,
        user_identity: Option<&str>,
        granted_scopes: Option<&str>,
    ) -> Result<CredentialVaultEntry> {
        let id = Uuid::new_v4();
        let now = chrono::Utc::now().to_rfc3339();
        let entry = CredentialVaultEntry {
            id,
            tenant_id,
            agent_id,
            name: name.to_string(),
            kind: kind.to_string(),
            encrypted_value: encrypted_value.to_vec(),
            provider: provider.map(|s| s.to_string()),
            user_identity: user_identity.map(|s| s.to_string()),
            granted_scopes: granted_scopes.map(|s| s.to_string()),
            created_at: now.clone(),
            updated_at: now,
            rotated_at: None,
        };
        conn.execute(
            "INSERT INTO credential_vault_entries (id, tenant_id, agent_id, name, kind,
             encrypted_value, provider, user_identity, granted_scopes, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                entry.id.to_string(),
                entry.tenant_id.to_string(),
                entry.agent_id.map(|u| u.to_string()),
                entry.name,
                entry.kind,
                entry.encrypted_value,
                entry.provider,
                entry.user_identity,
                entry.granted_scopes,
                entry.created_at,
                entry.updated_at,
            ],
        )?;
        Ok(entry)
    }

    /// Look up a credential by name: search agent-level first, then tenant-level (agent_id IS NULL).
    pub fn get_by_name(
        conn: &Connection,
        tenant_id: Uuid,
        name: &str,
        agent_id: Option<Uuid>,
    ) -> Result<Option<CredentialVaultEntry>> {
        // Try agent-level first if agent_id provided
        if let Some(aid) = agent_id {
            let sql = format!(
                "SELECT {} FROM credential_vault_entries WHERE tenant_id = ?1 AND name = ?2 AND agent_id = ?3",
                SELECT_COLS
            );
            let mut stmt = conn.prepare(&sql)?;
            let mut rows = stmt.query_map(
                params![tenant_id.to_string(), name, aid.to_string()],
                row_to_credential,
            )?;
            if let Some(row) = rows.next() {
                return Ok(Some(row?));
            }
        }
        // Fall back to tenant-level
        let sql = format!(
            "SELECT {} FROM credential_vault_entries WHERE tenant_id = ?1 AND name = ?2 AND agent_id IS NULL",
            SELECT_COLS
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query_map(params![tenant_id.to_string(), name], row_to_credential)?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    pub fn list(conn: &Connection, tenant_id: Uuid) -> Result<Vec<CredentialVaultEntry>> {
        let sql = format!(
            "SELECT {} FROM credential_vault_entries WHERE tenant_id = ?1 ORDER BY name",
            SELECT_COLS
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params![tenant_id.to_string()], row_to_credential)?;
        let mut entries = Vec::new();
        for row in rows {
            entries.push(row?);
        }
        Ok(entries)
    }

    pub fn update_encrypted_value(
        conn: &Connection,
        id: Uuid,
        encrypted_value: &[u8],
    ) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE credential_vault_entries SET encrypted_value = ?1, updated_at = ?2, rotated_at = ?2
             WHERE id = ?3",
            params![encrypted_value, now, id.to_string()],
        )?;
        Ok(())
    }

    pub fn delete(conn: &Connection, id: Uuid) -> Result<()> {
        conn.execute(
            "DELETE FROM credential_vault_entries WHERE id = ?1",
            params![id.to_string()],
        )?;
        Ok(())
    }
}
