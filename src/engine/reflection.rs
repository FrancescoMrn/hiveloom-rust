use crate::store::models::{Agent, ReflectionReport};
use rusqlite::Connection;
use uuid::Uuid;

/// Run a reflection pass for an agent, analyzing recent capability invocations
/// and memory entries to produce skill and memory suggestions.
///
/// This is a local analysis (no LLM call) that examines the tenant-scoped logs
/// and memories within the given time window.
pub fn run_reflection(
    conn: &Connection,
    tenant_id: Uuid,
    agent: &Agent,
    trigger: &str,
    window_start: &str,
    window_end: &str,
) -> anyhow::Result<ReflectionReport> {
    // Gather capability invocation stats within the window
    let mut stmt = conn.prepare(
        "SELECT capability_id, COUNT(*) as cnt,
                SUM(CASE WHEN success = 1 THEN 1 ELSE 0 END) as ok_cnt,
                AVG(latency_ms) as avg_latency
         FROM capability_invocation_logs
         WHERE tenant_id = ?1 AND agent_id = ?2
           AND created_at >= ?3 AND created_at <= ?4
         GROUP BY capability_id",
    )?;

    let mut skill_suggestions: Vec<serde_json::Value> = Vec::new();

    let rows = stmt.query_map(
        rusqlite::params![
            tenant_id.to_string(),
            agent.id.to_string(),
            window_start,
            window_end,
        ],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, f64>(3)?,
            ))
        },
    )?;

    for row in rows {
        let (cap_id, total, ok, avg_latency) = row?;
        let fail_rate = if total > 0 {
            ((total - ok) as f64) / (total as f64)
        } else {
            0.0
        };

        if fail_rate > 0.3 {
            skill_suggestions.push(serde_json::json!({
                "type": "high_failure_rate",
                "capability_id": cap_id,
                "total_invocations": total,
                "failure_rate": format!("{:.0}%", fail_rate * 100.0),
                "suggestion": "Review capability configuration or endpoint availability.",
            }));
        }

        if avg_latency > 5000.0 {
            skill_suggestions.push(serde_json::json!({
                "type": "high_latency",
                "capability_id": cap_id,
                "avg_latency_ms": avg_latency as i64,
                "suggestion": "Consider caching or optimizing the external endpoint.",
            }));
        }
    }

    // Gather memory stats
    let mut memory_suggestions: Vec<serde_json::Value> = Vec::new();

    let memory_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM memory_entries
         WHERE tenant_id = ?1 AND agent_id = ?2 AND archived = 0",
        rusqlite::params![tenant_id.to_string(), agent.id.to_string()],
        |row| row.get(0),
    )?;

    if memory_count > 100 {
        memory_suggestions.push(serde_json::json!({
            "type": "high_memory_count",
            "count": memory_count,
            "suggestion": "Consider archiving older or low-confidence memory entries.",
        }));
    }

    // Check for coerced entries
    let coerced_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM memory_entries
         WHERE tenant_id = ?1 AND agent_id = ?2 AND archived = 0 AND coerced = 1",
        rusqlite::params![tenant_id.to_string(), agent.id.to_string()],
        |row| row.get(0),
    )?;

    if coerced_count > 10 {
        memory_suggestions.push(serde_json::json!({
            "type": "many_coerced_entries",
            "count": coerced_count,
            "suggestion": "Many memory entries were scope-coerced. Review scope_mode settings.",
        }));
    }

    let report = ReflectionReport::create(
        conn,
        tenant_id,
        agent.id,
        trigger,
        window_start,
        window_end,
        &serde_json::to_string(&skill_suggestions)?,
        &serde_json::to_string(&memory_suggestions)?,
    )?;

    Ok(report)
}
