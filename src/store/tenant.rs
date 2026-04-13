use rusqlite::Connection;
use std::path::Path;

use crate::store::migrations::run_migrations;

/// Migrations applied to every per-tenant database.
const TENANT_MIGRATIONS: &[(&str, &str)] = &[
    ("0001_create_agents", r#"
        CREATE TABLE IF NOT EXISTS agents (
            id TEXT NOT NULL,
            tenant_id TEXT NOT NULL,
            name TEXT NOT NULL,
            system_prompt TEXT NOT NULL DEFAULT '',
            model_id TEXT NOT NULL,
            scope_mode TEXT NOT NULL DEFAULT 'dual' CHECK(scope_mode IN ('dual','tenant-only','user-only')),
            default_scope_policy TEXT NOT NULL DEFAULT 'tenant' CHECK(default_scope_policy IN ('tenant','user')),
            scope_coerce_policy TEXT NOT NULL DEFAULT 'coerce' CHECK(scope_coerce_policy IN ('coerce','drop')),
            reflection_enabled INTEGER NOT NULL DEFAULT 0,
            reflection_cron TEXT,
            status TEXT NOT NULL DEFAULT 'active' CHECK(status IN ('active','disabled')),
            version INTEGER NOT NULL DEFAULT 1,
            is_current INTEGER NOT NULL DEFAULT 1,
            parent_version_id TEXT,
            created_at TEXT NOT NULL,
            PRIMARY KEY (id, version)
        );
        CREATE INDEX IF NOT EXISTS idx_agents_current ON agents(tenant_id, is_current) WHERE is_current = 1;
    "#),
    ("0002_create_capabilities", r#"
        CREATE TABLE IF NOT EXISTS capabilities (
            id TEXT PRIMARY KEY,
            tenant_id TEXT NOT NULL,
            agent_id TEXT NOT NULL,
            name TEXT NOT NULL,
            description TEXT NOT NULL DEFAULT '',
            endpoint_url TEXT NOT NULL,
            auth_type TEXT NOT NULL DEFAULT 'none' CHECK(auth_type IN ('none','api_key','oauth')),
            credential_ref TEXT,
            input_schema TEXT,
            output_schema TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            UNIQUE(tenant_id, agent_id, name)
        );
    "#),
    ("0003_create_conversations", r#"
        CREATE TABLE IF NOT EXISTS conversations (
            id TEXT PRIMARY KEY,
            tenant_id TEXT NOT NULL,
            agent_id TEXT NOT NULL,
            surface_type TEXT NOT NULL CHECK(surface_type IN ('slack','mcp','internal')),
            surface_ref TEXT NOT NULL,
            user_identity TEXT NOT NULL,
            thread_ref TEXT,
            status TEXT NOT NULL DEFAULT 'active' CHECK(status IN ('active','concluded','abandoned')),
            workflow_state TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            concluded_at TEXT,
            abandoned_at TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_conversations_active ON conversations(tenant_id, agent_id, surface_type, surface_ref, status) WHERE status = 'active';
    "#),
    ("0004_create_conversation_turns", r#"
        CREATE TABLE IF NOT EXISTS conversation_turns (
            id TEXT PRIMARY KEY,
            tenant_id TEXT NOT NULL,
            conversation_id TEXT NOT NULL REFERENCES conversations(id),
            turn_index INTEGER NOT NULL,
            role TEXT NOT NULL CHECK(role IN ('user','assistant','tool_result','system')),
            content TEXT NOT NULL,
            token_count INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_turns_by_conversation ON conversation_turns(conversation_id, turn_index);
    "#),
    ("0005_create_memory_entries", r#"
        CREATE TABLE IF NOT EXISTS memory_entries (
            id TEXT PRIMARY KEY,
            tenant_id TEXT NOT NULL,
            agent_id TEXT NOT NULL,
            scope TEXT NOT NULL,
            key TEXT NOT NULL,
            value TEXT NOT NULL,
            source_conversation_id TEXT,
            confidence REAL DEFAULT 1.0,
            coerced INTEGER NOT NULL DEFAULT 0,
            coerced_from_scope TEXT,
            archived INTEGER NOT NULL DEFAULT 0,
            archived_at TEXT,
            expires_at TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            UNIQUE(tenant_id, agent_id, scope, key)
        );
        CREATE INDEX IF NOT EXISTS idx_memory_scope ON memory_entries(tenant_id, agent_id, scope) WHERE archived = 0;
    "#),
    ("0006_create_credential_vault_entries", r#"
        CREATE TABLE IF NOT EXISTS credential_vault_entries (
            id TEXT PRIMARY KEY,
            tenant_id TEXT NOT NULL,
            agent_id TEXT,
            name TEXT NOT NULL,
            kind TEXT NOT NULL CHECK(kind IN ('static','delegated_user_token')),
            encrypted_value BLOB NOT NULL,
            provider TEXT,
            user_identity TEXT,
            granted_scopes TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            rotated_at TEXT
        );
        CREATE INDEX IF NOT EXISTS idx_credentials_lookup ON credential_vault_entries(tenant_id, name, agent_id);
    "#),
    ("0007_create_chat_surface_bindings", r#"
        CREATE TABLE IF NOT EXISTS chat_surface_bindings (
            id TEXT PRIMARY KEY,
            tenant_id TEXT NOT NULL,
            agent_id TEXT NOT NULL,
            surface_type TEXT NOT NULL CHECK(surface_type IN ('slack','mcp')),
            surface_ref TEXT NOT NULL,
            created_at TEXT NOT NULL,
            UNIQUE(tenant_id, surface_type, surface_ref)
        );
    "#),
    ("0008_create_dedup_entries", r#"
        CREATE TABLE IF NOT EXISTS dedup_entries (
            delivery_id TEXT PRIMARY KEY,
            tenant_id TEXT NOT NULL,
            surface_type TEXT NOT NULL,
            received_at TEXT NOT NULL,
            expires_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_dedup_expiry ON dedup_entries(expires_at);
    "#),
    ("0009_create_capability_invocation_logs", r#"
        CREATE TABLE IF NOT EXISTS capability_invocation_logs (
            id TEXT PRIMARY KEY,
            tenant_id TEXT NOT NULL,
            agent_id TEXT NOT NULL,
            capability_id TEXT NOT NULL,
            conversation_id TEXT,
            success INTEGER NOT NULL,
            latency_ms INTEGER NOT NULL,
            error_message TEXT,
            created_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_invocation_logs_recent ON capability_invocation_logs(tenant_id, agent_id, created_at);
    "#),
    ("0010_create_scheduled_jobs", r#"
        CREATE TABLE IF NOT EXISTS scheduled_jobs (
            id TEXT PRIMARY KEY,
            tenant_id TEXT NOT NULL,
            agent_id TEXT NOT NULL,
            cron_expression TEXT,
            one_time_at TEXT,
            timezone TEXT NOT NULL DEFAULT 'UTC',
            initial_context TEXT NOT NULL DEFAULT '',
            status TEXT NOT NULL DEFAULT 'active' CHECK(status IN ('active','paused','disabled')),
            last_fired_at TEXT,
            next_fire_at TEXT,
            created_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_scheduled_jobs_due ON scheduled_jobs(status, next_fire_at);
    "#),
    ("0011_create_event_subscriptions", r#"
        CREATE TABLE IF NOT EXISTS event_subscriptions (
            id TEXT PRIMARY KEY,
            tenant_id TEXT NOT NULL,
            agent_id TEXT NOT NULL,
            event_type TEXT NOT NULL,
            source_filter TEXT,
            auth_token_hash TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'active' CHECK(status IN ('active','disabled')),
            created_at TEXT NOT NULL
        );
    "#),
    // ── Phase 6: OAuth consent flow (T072) ─────────────────────────────
    ("0012_create_oauth_authorization_requests", r#"
        CREATE TABLE IF NOT EXISTS oauth_authorization_requests (
            id TEXT PRIMARY KEY,
            tenant_id TEXT NOT NULL,
            user_identity TEXT NOT NULL,
            provider TEXT NOT NULL,
            state_token TEXT NOT NULL UNIQUE,
            requested_scopes TEXT,
            paused_run_ref TEXT,
            surface_type TEXT,
            expires_at TEXT NOT NULL,
            completed_at TEXT,
            created_at TEXT NOT NULL
        );
    "#),
    // ── Phase 6: MCP tables (T078) ─────────────────────────────────────
    ("0013_create_mcp_identities", r#"
        CREATE TABLE IF NOT EXISTS mcp_identities (
            id TEXT PRIMARY KEY,
            tenant_id TEXT NOT NULL,
            name TEXT NOT NULL,
            mapped_person_id TEXT,
            status TEXT NOT NULL DEFAULT 'active' CHECK(status IN ('active','disabled','revoked')),
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
    "#),
    ("0014_create_mcp_client_registrations", r#"
        CREATE TABLE IF NOT EXISTS mcp_client_registrations (
            id TEXT PRIMARY KEY,
            tenant_id TEXT NOT NULL,
            mcp_identity_id TEXT NOT NULL REFERENCES mcp_identities(id),
            client_id TEXT NOT NULL UNIQUE,
            access_token_hash TEXT NOT NULL,
            refresh_token_hash TEXT,
            token_expires_at TEXT,
            created_at TEXT NOT NULL,
            revoked_at TEXT
        );
    "#),
    // ── Phase 7: Reflection reports (T097) ──────────────────────────────
    ("0016_create_reflection_reports", r#"
        CREATE TABLE IF NOT EXISTS reflection_reports (
            id TEXT PRIMARY KEY,
            tenant_id TEXT NOT NULL,
            agent_id TEXT NOT NULL,
            trigger TEXT NOT NULL CHECK(trigger IN ('scheduled','manual')),
            window_start TEXT NOT NULL,
            window_end TEXT NOT NULL,
            skill_suggestions TEXT NOT NULL DEFAULT '[]',
            memory_suggestions TEXT NOT NULL DEFAULT '[]',
            created_at TEXT NOT NULL
        );
    "#),
    ("0015_create_mcp_setup_codes", r#"
        CREATE TABLE IF NOT EXISTS mcp_setup_codes (
            id TEXT PRIMARY KEY,
            tenant_id TEXT NOT NULL,
            mcp_identity_id TEXT NOT NULL REFERENCES mcp_identities(id),
            code_hash TEXT NOT NULL,
            expires_at TEXT NOT NULL,
            used_at TEXT,
            created_at TEXT NOT NULL
        );
    "#),
];

pub struct TenantStore {
    conn: Connection,
}

impl TenantStore {
    /// Open (or create) the per-tenant database at
    /// `<data_dir>/tenants/<tenant_id>/store.db`.
    pub fn open(data_dir: &Path, tenant_id: &uuid::Uuid) -> anyhow::Result<Self> {
        let tenant_dir = data_dir.join("tenants").join(tenant_id.to_string());
        std::fs::create_dir_all(&tenant_dir)?;
        let db_path = tenant_dir.join("store.db");
        let conn = Connection::open(&db_path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        run_migrations(&conn, TENANT_MIGRATIONS)?;
        Ok(Self { conn })
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }
}
