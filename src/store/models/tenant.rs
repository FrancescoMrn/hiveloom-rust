use anyhow::Result;
use rusqlite::{params, Connection};
use uuid::Uuid;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Tenant {
    pub id: Uuid,
    pub name: String,
    pub slug: String,
    pub timezone: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

impl Tenant {
    pub fn create(conn: &Connection, name: &str, slug: &str, timezone: &str) -> Result<Tenant> {
        let id = Uuid::new_v4();
        let now = chrono::Utc::now().to_rfc3339();
        let tenant = Tenant {
            id,
            name: name.to_string(),
            slug: slug.to_string(),
            timezone: timezone.to_string(),
            status: "active".to_string(),
            created_at: now.clone(),
            updated_at: now,
        };
        conn.execute(
            "INSERT INTO tenants (id, name, slug, timezone, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                tenant.id.to_string(),
                tenant.name,
                tenant.slug,
                tenant.timezone,
                tenant.status,
                tenant.created_at,
                tenant.updated_at,
            ],
        )?;
        Ok(tenant)
    }

    pub fn get_by_id(conn: &Connection, id: Uuid) -> Result<Option<Tenant>> {
        let mut stmt = conn.prepare(
            "SELECT id, name, slug, timezone, status, created_at, updated_at
             FROM tenants WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(params![id.to_string()], |row| {
            Ok(Tenant {
                id: row.get::<_, String>(0)?.parse().unwrap(),
                name: row.get(1)?,
                slug: row.get(2)?,
                timezone: row.get(3)?,
                status: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    pub fn get_by_slug(conn: &Connection, slug: &str) -> Result<Option<Tenant>> {
        let mut stmt = conn.prepare(
            "SELECT id, name, slug, timezone, status, created_at, updated_at
             FROM tenants WHERE slug = ?1",
        )?;
        let mut rows = stmt.query_map(params![slug], |row| {
            Ok(Tenant {
                id: row.get::<_, String>(0)?.parse().unwrap(),
                name: row.get(1)?,
                slug: row.get(2)?,
                timezone: row.get(3)?,
                status: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    pub fn list(conn: &Connection) -> Result<Vec<Tenant>> {
        let mut stmt = conn.prepare(
            "SELECT id, name, slug, timezone, status, created_at, updated_at
             FROM tenants ORDER BY created_at",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Tenant {
                id: row.get::<_, String>(0)?.parse().unwrap(),
                name: row.get(1)?,
                slug: row.get(2)?,
                timezone: row.get(3)?,
                status: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })?;
        let mut tenants = Vec::new();
        for row in rows {
            tenants.push(row?);
        }
        Ok(tenants)
    }

    pub fn update_status(conn: &Connection, id: Uuid, status: &str) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE tenants SET status = ?1, updated_at = ?2 WHERE id = ?3",
            params![status, now, id.to_string()],
        )?;
        Ok(())
    }

    pub fn delete(conn: &Connection, id: Uuid) -> Result<()> {
        Self::update_status(conn, id, "deleted")
    }
}
