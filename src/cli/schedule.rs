use clap::{Args, Subcommand};
use serde::{Deserialize, Serialize};

use super::client::ApiClient;

#[derive(Args)]
pub struct ScheduleArgs {
    #[command(subcommand)]
    pub command: ScheduleCommand,

    /// Tenant slug (default: "default")
    #[arg(long, default_value_t = crate::cli::local::default_tenant(), global = true)]
    pub tenant: String,

    /// API endpoint
    #[arg(long, global = true)]
    pub endpoint: Option<String>,

    /// Bearer token for remote API access
    #[arg(long, global = true)]
    pub token: Option<String>,

    /// Output as JSON instead of table
    #[arg(long, global = true)]
    pub json: bool,
}

#[derive(Subcommand)]
pub enum ScheduleCommand {
    /// Create a new scheduled job for an agent
    Create {
        /// Agent ID
        agent: String,
        /// Cron expression (5-field standard cron or 6-field cron with seconds)
        #[arg(long)]
        cron: Option<String>,
        /// One-time fire at (RFC3339 datetime)
        #[arg(long)]
        one_time_at: Option<String>,
        /// Timezone (default: UTC)
        #[arg(long, default_value = "UTC")]
        timezone: String,
        /// Initial context message for the agent
        #[arg(long, default_value = "")]
        context: String,
    },
    /// List scheduled jobs for an agent
    List {
        /// Agent ID
        agent: String,
    },
    /// Show scheduled job details
    Show {
        /// Agent ID
        agent: String,
        /// Job ID
        job: String,
    },
    /// Pause a scheduled job
    Pause {
        /// Agent ID
        agent: String,
        /// Job ID
        job: String,
    },
    /// Resume a paused scheduled job
    Resume {
        /// Agent ID
        agent: String,
        /// Job ID
        job: String,
    },
    /// Delete a scheduled job
    Delete {
        /// Agent ID
        agent: String,
        /// Job ID
        job: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct ScheduledJobResponse {
    pub id: String,
    #[serde(default)]
    pub tenant_id: String,
    #[serde(default)]
    pub agent_id: String,
    pub cron_expression: Option<String>,
    pub one_time_at: Option<String>,
    #[serde(default)]
    pub timezone: String,
    #[serde(default)]
    pub initial_context: String,
    #[serde(default)]
    pub status: String,
    pub last_fired_at: Option<String>,
    pub next_fire_at: Option<String>,
    #[serde(default)]
    pub created_at: String,
}

pub async fn run(args: ScheduleArgs) -> anyhow::Result<()> {
    let client = ApiClient::new(args.endpoint.clone(), args.token.clone());
    let tid = &args.tenant;
    let json_out = args.json;

    match args.command {
        ScheduleCommand::Create {
            agent,
            cron,
            one_time_at,
            timezone,
            context,
        } => {
            let body = serde_json::json!({
                "cron_expression": cron,
                "one_time_at": one_time_at,
                "timezone": timezone,
                "initial_context": context,
            });
            let job: ScheduledJobResponse = client
                .post(
                    &format!("/api/tenants/{tid}/agents/{agent}/scheduled-jobs"),
                    &body,
                )
                .await?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&job)?);
            } else {
                println!("Created scheduled job {} (status: {})", job.id, job.status);
                if let Some(ref next) = job.next_fire_at {
                    println!("Next fire at: {next}");
                }
            }
        }
        ScheduleCommand::List { agent } => {
            let jobs: Vec<ScheduledJobResponse> = client
                .get(&format!("/api/tenants/{tid}/agents/{agent}/scheduled-jobs"))
                .await?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&jobs)?);
            } else {
                println!(
                    "{:<38} {:<10} {:<24} {:<30}",
                    "ID", "STATUS", "CRON", "NEXT FIRE"
                );
                println!("{}", "-".repeat(104));
                for j in &jobs {
                    println!(
                        "{:<38} {:<10} {:<24} {:<30}",
                        j.id,
                        j.status,
                        j.cron_expression.as_deref().unwrap_or("-"),
                        j.next_fire_at.as_deref().unwrap_or("-"),
                    );
                }
                println!("\n{} job(s)", jobs.len());
            }
        }
        ScheduleCommand::Show { agent, job } => {
            let j: ScheduledJobResponse = client
                .get(&format!(
                    "/api/tenants/{tid}/agents/{agent}/scheduled-jobs/{job}"
                ))
                .await?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&j)?);
            } else {
                println!("Job ID:       {}", j.id);
                println!("Agent:        {}", j.agent_id);
                println!("Tenant:       {}", j.tenant_id);
                println!(
                    "Cron:         {}",
                    j.cron_expression.as_deref().unwrap_or("-")
                );
                println!("One-time at:  {}", j.one_time_at.as_deref().unwrap_or("-"));
                println!("Timezone:     {}", j.timezone);
                println!("Status:       {}", j.status);
                println!("Next fire:    {}", j.next_fire_at.as_deref().unwrap_or("-"));
                println!(
                    "Last fired:   {}",
                    j.last_fired_at.as_deref().unwrap_or("-")
                );
                println!("Created:      {}", j.created_at);
                if !j.initial_context.is_empty() {
                    println!("Context:");
                    for line in j.initial_context.lines() {
                        println!("  {line}");
                    }
                }
            }
        }
        ScheduleCommand::Pause { agent, job } => {
            let j: ScheduledJobResponse = client
                .post(
                    &format!("/api/tenants/{tid}/agents/{agent}/scheduled-jobs/{job}/pause"),
                    &serde_json::json!({}),
                )
                .await?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&j)?);
            } else {
                println!("Paused job {}", j.id);
            }
        }
        ScheduleCommand::Resume { agent, job } => {
            let j: ScheduledJobResponse = client
                .post(
                    &format!("/api/tenants/{tid}/agents/{agent}/scheduled-jobs/{job}/resume"),
                    &serde_json::json!({}),
                )
                .await?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&j)?);
            } else {
                println!(
                    "Resumed job {} (next fire: {})",
                    j.id,
                    j.next_fire_at.as_deref().unwrap_or("-")
                );
            }
        }
        ScheduleCommand::Delete { agent, job } => {
            client
                .delete(&format!(
                    "/api/tenants/{tid}/agents/{agent}/scheduled-jobs/{job}"
                ))
                .await?;
            println!("Deleted scheduled job {job}");
        }
    }

    Ok(())
}
