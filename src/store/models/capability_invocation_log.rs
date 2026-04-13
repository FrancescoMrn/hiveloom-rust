use anyhow::Result;
use rusqlite::{params, Connection};
use uuid::Uuid;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CapabilityInvocationLog {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub agent_id: Uuid,
    pub capability_id: Uuid,
    pub conversation_id: Option<Uuid>,
    pub success: bool,
    pub latency_ms: i64,
    pub error_message: Option<String>,
    pub created_at: String,
}

impl CapabilityInvocationLog {
    pub fn create(
        conn: &Connection,
        tenant_id: Uuid,
        agent_id: Uuid,
        capability_id: Uuid,
        conversation_id: Option<Uuid>,
        success: bool,
        latency_ms: i64,
        error_message: Option<&str>,
    ) -> Result<CapabilityInvocationLog> {
        let id = Uuid::new_v4();
        let now = chrono::Utc::now().to_rfc3339();
        let log = CapabilityInvocationLog {
            id,
            tenant_id,
            agent_id,
            capability_id,
            conversation_id,
            success,
            latency_ms,
            error_message: error_message.map(|s| s.to_string()),
            created_at: now,
        };
        conn.execute(
            "INSERT INTO capability_invocation_logs (id, tenant_id, agent_id, capability_id,
             conversation_id, success, latency_ms, error_message, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                log.id.to_string(),
                log.tenant_id.to_string(),
                log.agent_id.to_string(),
                log.capability_id.to_string(),
                log.conversation_id.map(|u| u.to_string()),
                log.success as i64,
                log.latency_ms,
                log.error_message,
                log.created_at,
            ],
        )?;
        Ok(log)
    }
}
