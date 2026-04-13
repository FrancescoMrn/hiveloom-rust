use rusqlite::Connection;
use std::path::Path;
use std::sync::Mutex;

use crate::store::migrations::run_migrations;

/// Migrations applied to every platform database.
const PLATFORM_MIGRATIONS: &[(&str, &str)] = &[
    ("0001_create_tenants", r#"
        CREATE TABLE IF NOT EXISTS tenants (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            slug TEXT NOT NULL UNIQUE,
            timezone TEXT NOT NULL DEFAULT 'UTC',
            status TEXT NOT NULL DEFAULT 'active' CHECK(status IN ('active','disabled','deleted')),
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
    "#),
    ("0002_create_routing_assignments", r#"
        CREATE TABLE IF NOT EXISTS routing_assignments (
            tenant_id TEXT NOT NULL REFERENCES tenants(id),
            instance_id TEXT NOT NULL,
            assigned_at TEXT NOT NULL,
            PRIMARY KEY (tenant_id)
        );
    "#),
    ("0003_create_platform_admin_tokens", r#"
        CREATE TABLE IF NOT EXISTS platform_admin_tokens (
            id TEXT PRIMARY KEY,
            token_hash TEXT NOT NULL UNIQUE,
            scope TEXT NOT NULL,
            created_at TEXT NOT NULL,
            expires_at TEXT,
            revoked_at TEXT
        );
    "#),
];

pub struct PlatformStore {
    conn: Mutex<Connection>,
}

impl PlatformStore {
    /// Open (or create) the platform database at `<data_dir>/platform.db`.
    pub fn open(data_dir: &Path) -> anyhow::Result<Self> {
        std::fs::create_dir_all(data_dir)?;
        let db_path = data_dir.join("platform.db");
        let conn = Connection::open(&db_path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        run_migrations(&conn, PLATFORM_MIGRATIONS)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    /// Acquire a lock on the platform database connection.
    pub fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().expect("platform store mutex poisoned")
    }

    /// Auto-provision the default tenant if the platform store is empty (FR-032/FR-033).
    /// Returns the default tenant's ID if provisioned or already exists.
    pub fn ensure_default_tenant(&self) -> anyhow::Result<uuid::Uuid> {
        use crate::store::models::Tenant;
        let conn = self.conn();
        if let Some(t) = Tenant::get_by_slug(&conn, "default")? {
            return Ok(t.id);
        }
        let tenant = Tenant::create(&conn, "Default", "default", "UTC")?;
        tracing::info!(tenant_id = %tenant.id, "Auto-provisioned default tenant");
        Ok(tenant.id)
    }
}
