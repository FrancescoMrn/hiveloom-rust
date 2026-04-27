use chrono::{DateTime, Utc};
use cron::Schedule;
use std::collections::HashSet;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use crate::compaction::engine::cleanup_expired_archives;
use crate::compaction::event::CompactionEvent;
use crate::store::models::{Agent, Capability, Conversation, CredentialVaultEntry, ScheduledJob};
use crate::store::{TenantStore, Vault};

pub struct JobScheduler {
    data_dir: String,
    /// Per-agent concurrency mutex (T058): tracks agents currently running.
    running_agents: Arc<Mutex<HashSet<Uuid>>>,
}

impl JobScheduler {
    pub fn new(data_dir: &str) -> Self {
        Self {
            data_dir: data_dir.to_string(),
            running_agents: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Main scheduler loop: runs as a tokio task.
    /// - Scans all tenants for active scheduled jobs
    /// - Computes next_fire_at for each using cron + timezone
    /// - Sleeps until the earliest, then fires
    /// - After firing, recomputes next_fire_at
    pub async fn run(&self) -> anyhow::Result<()> {
        tracing::info!("Scheduler starting");

        loop {
            let now = Utc::now();
            let now_str = now.to_rfc3339();

            // Scan tenant directories for due jobs
            let tenants_dir = std::path::Path::new(&self.data_dir).join("tenants");
            if !tenants_dir.exists() {
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                continue;
            }

            let mut earliest_next: Option<DateTime<Utc>> = None;

            let entries = match std::fs::read_dir(&tenants_dir) {
                Ok(e) => e,
                Err(_) => {
                    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                    continue;
                }
            };

            for entry in entries.flatten() {
                let tenant_id_str = entry.file_name().to_string_lossy().to_string();
                let tenant_id = match Uuid::parse_str(&tenant_id_str) {
                    Ok(id) => id,
                    Err(_) => continue,
                };

                let store =
                    match TenantStore::open(std::path::Path::new(&self.data_dir), &tenant_id) {
                        Ok(s) => s,
                        Err(_) => continue,
                    };
                let conn = store.conn();

                // First, ensure all active jobs have a computed next_fire_at
                if let Ok(active_jobs) = ScheduledJob::list_active(conn) {
                    for job in &active_jobs {
                        if job.next_fire_at.is_none() {
                            if let Some(ref cron_expr) = job.cron_expression {
                                if let Ok(next) = compute_next_fire(cron_expr, &job.timezone, now) {
                                    let next_str = next.to_rfc3339();
                                    let _ = ScheduledJob::update_next_fire(
                                        conn,
                                        job.id,
                                        Some(&next_str),
                                    );
                                }
                            } else if let Some(ref one_time) = job.one_time_at {
                                let _ =
                                    ScheduledJob::update_next_fire(conn, job.id, Some(one_time));
                            }
                        }
                    }
                }

                // Now find due jobs
                let due_jobs = match ScheduledJob::list_due(conn, &now_str) {
                    Ok(jobs) => jobs,
                    Err(_) => continue,
                };

                for job in due_jobs {
                    // T058: check per-agent concurrency
                    {
                        let mut running = self
                            .running_agents
                            .lock()
                            .map_err(|e| anyhow::anyhow!("scheduler lock poisoned: {}", e))?;
                        if running.contains(&job.agent_id) {
                            tracing::debug!(
                                agent_id = %job.agent_id,
                                job_id = %job.id,
                                "Skipping job: agent already running"
                            );
                            continue;
                        }
                        running.insert(job.agent_id);
                    }

                    // Update last_fired_at
                    let _ = ScheduledJob::update_last_fired(conn, job.id, &now_str);

                    // Compute and update next_fire_at
                    if let Some(ref cron_expr) = job.cron_expression {
                        match compute_next_fire(cron_expr, &job.timezone, now) {
                            Ok(next) => {
                                let next_str = next.to_rfc3339();
                                let _ =
                                    ScheduledJob::update_next_fire(conn, job.id, Some(&next_str));

                                // Track for earliest next
                                match earliest_next {
                                    None => earliest_next = Some(next),
                                    Some(e) if next < e => earliest_next = Some(next),
                                    _ => {}
                                }
                            }
                            Err(e) => {
                                tracing::warn!(job_id = %job.id, error = %e, "Failed to compute next fire time");
                            }
                        }
                    } else {
                        // One-time job: disable after firing
                        let _ = ScheduledJob::disable(conn, job.id);
                    }

                    // Fire the job in a background task
                    let data_dir = self.data_dir.clone();
                    let running_agents = self.running_agents.clone();
                    let job_clone = job.clone();
                    let tid = tenant_id;
                    tokio::spawn(async move {
                        if let Err(e) = fire_job(&data_dir, &job_clone, &tid).await {
                            tracing::error!(
                                job_id = %job_clone.id,
                                agent_id = %job_clone.agent_id,
                                error = %e,
                                "Scheduled job failed"
                            );
                        }
                        // Remove agent from running set
                        if let Ok(mut running) = running_agents.lock() {
                            running.remove(&job_clone.agent_id);
                        } else {
                            tracing::error!("Scheduler lock poisoned while clearing running agent");
                        }
                    });
                }

                // Track next fire times for remaining active jobs
                if let Ok(active_jobs) = ScheduledJob::list_active(conn) {
                    for job in &active_jobs {
                        if let Some(ref next_str) = job.next_fire_at {
                            if let Ok(next) = DateTime::parse_from_rfc3339(next_str) {
                                let next_utc = next.with_timezone(&Utc);
                                if next_utc > now {
                                    match earliest_next {
                                        None => earliest_next = Some(next_utc),
                                        Some(e) if next_utc < e => earliest_next = Some(next_utc),
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
                }

                // T033, T034: Periodically run compaction retention cleanup
                // (runs alongside scheduled job checks — lightweight DELETE queries)
                run_compaction_retention(conn);
            }

            // Sleep until next job is due, or a default poll interval
            let sleep_dur = match earliest_next {
                Some(next) => {
                    let dur = (next - Utc::now())
                        .to_std()
                        .unwrap_or(std::time::Duration::from_secs(1));
                    // Cap at 60 seconds to allow for newly-created jobs
                    dur.min(std::time::Duration::from_secs(60))
                }
                None => std::time::Duration::from_secs(30),
            };

            tokio::time::sleep(sleep_dur).await;
        }
    }
}

/// T057: Timezone-aware cron evaluation.
pub fn compute_next_fire(
    cron_expr: &str,
    timezone: &str,
    after: DateTime<Utc>,
) -> anyhow::Result<DateTime<Utc>> {
    let tz: chrono_tz::Tz = timezone
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid timezone '{}': {}", timezone, e))?;
    let normalized = normalize_cron_expression(cron_expr)?;
    let schedule = Schedule::from_str(&normalized)
        .map_err(|e| anyhow::anyhow!("Invalid cron expression '{}': {}", cron_expr, e))?;
    let after_in_tz = after.with_timezone(&tz);
    let next = schedule
        .after(&after_in_tz)
        .next()
        .ok_or_else(|| anyhow::anyhow!("No next fire time for cron expression"))?;
    Ok(next.with_timezone(&Utc))
}

fn normalize_cron_expression(cron_expr: &str) -> anyhow::Result<String> {
    let fields: Vec<&str> = cron_expr.split_whitespace().collect();
    match fields.len() {
        5 => Ok(format!("0 {}", fields.join(" "))),
        6 => Ok(fields.join(" ")),
        n => anyhow::bail!(
            "Invalid cron expression '{}': expected 5 fields (standard cron) or 6 fields (with seconds), got {}",
            cron_expr,
            n
        ),
    }
}

/// T033, T034: Clean up expired compaction artifacts (30-day retention).
fn run_compaction_retention(conn: &rusqlite::Connection) {
    // T033: Delete expired raw turn archive entries
    match cleanup_expired_archives(conn) {
        Ok(count) if count > 0 => {
            tracing::debug!(
                deleted = count,
                "Cleaned up expired raw turn archive entries"
            );
        }
        Err(e) => {
            tracing::warn!(error = %e, "Failed to clean up raw turn archive");
        }
        _ => {}
    }

    // T034: Delete expired compaction events (30-day retention)
    let cutoff = (chrono::Utc::now() - chrono::Duration::days(30)).to_rfc3339();
    match CompactionEvent::cleanup_expired(conn, &cutoff) {
        Ok(count) if count > 0 => {
            tracing::debug!(deleted = count, "Cleaned up expired compaction events");
        }
        Err(e) => {
            tracing::warn!(error = %e, "Failed to clean up compaction events");
        }
        _ => {}
    }
}

/// T061: Fire a single scheduled job: open tenant store, load agent, create
/// a synthetic internal conversation, and run the agent loop.
async fn fire_job(data_dir: &str, job: &ScheduledJob, tenant_id: &Uuid) -> anyhow::Result<()> {
    tracing::info!(
        job_id = %job.id,
        agent_id = %job.agent_id,
        tenant_id = %tenant_id,
        "Firing scheduled job"
    );

    let data_dir = data_dir.to_string();
    let job = job.clone();
    let tenant_id = *tenant_id;

    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let data_path = std::path::Path::new(&data_dir);
        let store = TenantStore::open(data_path, &tenant_id)?;
        let conn = store.conn();
        let vault = Vault::open(data_path)?;

        let agent = Agent::get_current(conn, tenant_id, job.agent_id)?
            .ok_or_else(|| anyhow::anyhow!("Agent {} not found", job.agent_id))?;
        if agent.status != "active" {
            tracing::debug!(
                job_id = %job.id,
                agent_id = %job.agent_id,
                "Skipping scheduled job for inactive agent"
            );
            return Ok(());
        }

        let capabilities = Capability::list_by_agent(conn, tenant_id, job.agent_id)?;
        let conversation = Conversation::create(
            conn,
            tenant_id,
            job.agent_id,
            "internal",
            &format!("scheduled-job:{}", job.id),
            "system",
            None,
        )?;

        let initial_message = if job.initial_context.is_empty() {
            "You are running as a scheduled autonomous agent. Execute your configured tasks."
                .to_string()
        } else {
            job.initial_context.clone()
        };

        let credential_name = if agent.model_id.starts_with("claude-") {
            "anthropic"
        } else {
            "openai"
        };
        let credential = CredentialVaultEntry::get_by_name(conn, tenant_id, credential_name, None)?
            .ok_or_else(|| anyhow::anyhow!("No LLM credential '{}' found", credential_name))?;
        let api_key = String::from_utf8(vault.decrypt(&credential.encrypted_value)?)?;
        let provider = crate::llm::resolve_provider(&agent.model_id, &api_key)?;

        let invocation = crate::engine::AgentInvocation {
            agent,
            capabilities,
            conversation_id: conversation.id,
            tenant_id,
            user_identity: "system".to_string(),
        };

        let rt = tokio::runtime::Handle::current();
        let result = rt.block_on(crate::engine::agent_loop::run_agent_loop_with_vault(
            &invocation,
            provider.as_ref(),
            conn,
            &initial_message,
            &vault,
        ))?;

        tracing::info!(
            job_id = %job.id,
            conversation_id = %conversation.id,
            tool_calls = ?result.tool_calls_made,
            "Scheduled job completed through agent loop"
        );

        Conversation::update_status(conn, conversation.id, "concluded")?;
        Ok(())
    })
    .await?
}
