//! CompactionConfig model and resolution logic.
//!
//! Configuration resolution order: agent-level override -> tenant default -> platform defaults.
//! Platform defaults: threshold_pct=80, max_summary_fraction_pct=30, protected_turn_count=4, show_indicator=false.

use anyhow::{bail, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Platform-level hardcoded defaults (FR-009, FR-013).
pub const DEFAULT_THRESHOLD_PCT: i64 = 80;
pub const DEFAULT_MAX_SUMMARY_FRACTION_PCT: i64 = 30;
pub const DEFAULT_PROTECTED_TURN_COUNT: i64 = 4;
pub const DEFAULT_SHOW_INDICATOR: bool = false;

/// Persistent compaction configuration scoped to a tenant + optional agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionConfig {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub agent_id: Option<Uuid>,
    pub threshold_pct: i64,
    pub max_summary_fraction_pct: i64,
    pub protected_turn_count: i64,
    pub show_indicator: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// Resolved configuration with source tracking for display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedCompactionConfig {
    pub threshold_pct: i64,
    pub max_summary_fraction_pct: i64,
    pub protected_turn_count: i64,
    pub show_indicator: bool,
    pub source: ConfigSource,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ConfigSource {
    AgentOverride,
    TenantDefault,
    PlatformDefault,
}

impl std::fmt::Display for ConfigSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigSource::AgentOverride => write!(f, "agent override"),
            ConfigSource::TenantDefault => write!(f, "tenant default"),
            ConfigSource::PlatformDefault => write!(f, "platform default"),
        }
    }
}

// ── Validation (T011) ──────────────────────────────────────────────────

/// Validate threshold_pct: must be 50..=100.
pub fn validate_threshold_pct(value: i64) -> Result<()> {
    if !(50..=100).contains(&value) {
        bail!(
            "threshold_pct must be between 50 and 100 (got {}). \
             Values below 50% cause premature compaction; values above 100% disable it.",
            value
        );
    }
    Ok(())
}

/// Validate max_summary_fraction_pct: must be 10..=50.
pub fn validate_max_summary_fraction_pct(value: i64) -> Result<()> {
    if !(10..=50).contains(&value) {
        bail!(
            "max_summary_fraction_pct must be between 10 and 50 (got {}). \
             Values below 10% lose too much context; values above 50% leave too little room for new turns.",
            value
        );
    }
    Ok(())
}

/// Validate protected_turn_count: must be 1..=20.
pub fn validate_protected_turn_count(value: i64) -> Result<()> {
    if !(1..=20).contains(&value) {
        bail!(
            "protected_turn_count must be between 1 and 20 (got {}). \
             At least 1 recent turn must be preserved.",
            value
        );
    }
    Ok(())
}

/// Validate all config fields at once.
pub fn validate_config(
    threshold_pct: i64,
    max_summary_fraction_pct: i64,
    protected_turn_count: i64,
) -> Result<()> {
    validate_threshold_pct(threshold_pct)?;
    validate_max_summary_fraction_pct(max_summary_fraction_pct)?;
    validate_protected_turn_count(protected_turn_count)?;
    Ok(())
}

// ── CRUD ───────────────────────────────────────────────────────────────

impl CompactionConfig {
    /// Create a new compaction config. Validates all fields.
    pub fn create(
        conn: &Connection,
        tenant_id: Uuid,
        agent_id: Option<Uuid>,
        threshold_pct: i64,
        max_summary_fraction_pct: i64,
        protected_turn_count: i64,
        show_indicator: bool,
    ) -> Result<Self> {
        validate_config(threshold_pct, max_summary_fraction_pct, protected_turn_count)?;

        let id = Uuid::new_v4();
        let now = chrono::Utc::now().to_rfc3339();
        let agent_id_str = agent_id.map(|a| a.to_string());

        conn.execute(
            "INSERT INTO compaction_config (id, tenant_id, agent_id, threshold_pct,
             max_summary_fraction_pct, protected_turn_count, show_indicator, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                id.to_string(),
                tenant_id.to_string(),
                agent_id_str,
                threshold_pct,
                max_summary_fraction_pct,
                protected_turn_count,
                show_indicator,
                now,
                now,
            ],
        )?;

        Ok(Self {
            id,
            tenant_id,
            agent_id,
            threshold_pct,
            max_summary_fraction_pct,
            protected_turn_count,
            show_indicator,
            created_at: now.clone(),
            updated_at: now,
        })
    }

    /// Get compaction config for a specific agent.
    pub fn get_for_agent(
        conn: &Connection,
        tenant_id: Uuid,
        agent_id: Uuid,
    ) -> Result<Option<Self>> {
        let mut stmt = conn.prepare(
            "SELECT id, tenant_id, agent_id, threshold_pct, max_summary_fraction_pct,
                    protected_turn_count, show_indicator, created_at, updated_at
             FROM compaction_config
             WHERE tenant_id = ?1 AND agent_id = ?2",
        )?;
        let mut rows = stmt.query_map(
            params![tenant_id.to_string(), agent_id.to_string()],
            row_to_config,
        )?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    /// Get tenant-level default config (agent_id IS NULL).
    pub fn get_tenant_default(conn: &Connection, tenant_id: Uuid) -> Result<Option<Self>> {
        let mut stmt = conn.prepare(
            "SELECT id, tenant_id, agent_id, threshold_pct, max_summary_fraction_pct,
                    protected_turn_count, show_indicator, created_at, updated_at
             FROM compaction_config
             WHERE tenant_id = ?1 AND agent_id IS NULL",
        )?;
        let mut rows = stmt.query_map(params![tenant_id.to_string()], row_to_config)?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    /// Update (patch) an existing config. Only non-None fields are updated.
    pub fn update(
        conn: &Connection,
        id: Uuid,
        threshold_pct: Option<i64>,
        max_summary_fraction_pct: Option<i64>,
        protected_turn_count: Option<i64>,
        show_indicator: Option<bool>,
    ) -> Result<()> {
        // Load current to merge
        let mut stmt = conn.prepare(
            "SELECT id, tenant_id, agent_id, threshold_pct, max_summary_fraction_pct,
                    protected_turn_count, show_indicator, created_at, updated_at
             FROM compaction_config WHERE id = ?1",
        )?;
        let current = stmt
            .query_row(params![id.to_string()], row_to_config)
            .map_err(|_| anyhow::anyhow!("Compaction config {} not found", id))?;

        let new_threshold = threshold_pct.unwrap_or(current.threshold_pct);
        let new_max_summary = max_summary_fraction_pct.unwrap_or(current.max_summary_fraction_pct);
        let new_protected = protected_turn_count.unwrap_or(current.protected_turn_count);
        let new_indicator = show_indicator.unwrap_or(current.show_indicator);

        validate_config(new_threshold, new_max_summary, new_protected)?;

        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE compaction_config
             SET threshold_pct = ?1, max_summary_fraction_pct = ?2,
                 protected_turn_count = ?3, show_indicator = ?4, updated_at = ?5
             WHERE id = ?6",
            params![new_threshold, new_max_summary, new_protected, new_indicator, now, id.to_string()],
        )?;
        Ok(())
    }

    /// Delete a compaction config (used for --reset to fall back to defaults).
    pub fn delete(conn: &Connection, id: Uuid) -> Result<()> {
        conn.execute(
            "DELETE FROM compaction_config WHERE id = ?1",
            params![id.to_string()],
        )?;
        Ok(())
    }

    /// Delete agent-level override, falling back to tenant/platform defaults.
    pub fn delete_for_agent(conn: &Connection, tenant_id: Uuid, agent_id: Uuid) -> Result<()> {
        conn.execute(
            "DELETE FROM compaction_config WHERE tenant_id = ?1 AND agent_id = ?2",
            params![tenant_id.to_string(), agent_id.to_string()],
        )?;
        Ok(())
    }
}

// ── Resolution (T010) ──────────────────────────────────────────────────

/// Resolve compaction config for an agent using the inheritance chain:
/// agent-level -> tenant default -> platform hardcoded defaults.
///
/// This is called on every compaction check (hot-reload: T027) so changes
/// take effect on the next conversation turn without restart.
pub fn resolve_config(
    conn: &Connection,
    tenant_id: Uuid,
    agent_id: Uuid,
) -> Result<ResolvedCompactionConfig> {
    // 1. Try agent-level override
    if let Some(cfg) = CompactionConfig::get_for_agent(conn, tenant_id, agent_id)? {
        return Ok(ResolvedCompactionConfig {
            threshold_pct: cfg.threshold_pct,
            max_summary_fraction_pct: cfg.max_summary_fraction_pct,
            protected_turn_count: cfg.protected_turn_count,
            show_indicator: cfg.show_indicator,
            source: ConfigSource::AgentOverride,
        });
    }

    // 2. Try tenant-level default
    if let Some(cfg) = CompactionConfig::get_tenant_default(conn, tenant_id)? {
        return Ok(ResolvedCompactionConfig {
            threshold_pct: cfg.threshold_pct,
            max_summary_fraction_pct: cfg.max_summary_fraction_pct,
            protected_turn_count: cfg.protected_turn_count,
            show_indicator: cfg.show_indicator,
            source: ConfigSource::TenantDefault,
        });
    }

    // 3. Platform hardcoded defaults
    Ok(ResolvedCompactionConfig {
        threshold_pct: DEFAULT_THRESHOLD_PCT,
        max_summary_fraction_pct: DEFAULT_MAX_SUMMARY_FRACTION_PCT,
        protected_turn_count: DEFAULT_PROTECTED_TURN_COUNT,
        show_indicator: DEFAULT_SHOW_INDICATOR,
        source: ConfigSource::PlatformDefault,
    })
}

fn row_to_config(row: &rusqlite::Row) -> rusqlite::Result<CompactionConfig> {
    let agent_id_str: Option<String> = row.get(2)?;
    Ok(CompactionConfig {
        id: row.get::<_, String>(0)?.parse().unwrap(),
        tenant_id: row.get::<_, String>(1)?.parse().unwrap(),
        agent_id: agent_id_str.and_then(|s| s.parse().ok()),
        threshold_pct: row.get(3)?,
        max_summary_fraction_pct: row.get(4)?,
        protected_turn_count: row.get(5)?,
        show_indicator: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}
