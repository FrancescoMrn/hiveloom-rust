use clap::Args;
use serde::{Deserialize, Serialize};

use super::client::ApiClient;

#[derive(Args)]
pub struct LogsArgs {
    /// Tenant slug
    #[arg(long, default_value = "default")]
    pub tenant: String,

    /// Agent ID to filter logs
    #[arg(long)]
    pub agent: Option<String>,

    /// Maximum number of log entries to show
    #[arg(long, default_value = "50")]
    pub limit: usize,

    /// API endpoint
    #[arg(long)]
    pub endpoint: Option<String>,

    /// Bearer token for remote API access
    #[arg(long)]
    pub token: Option<String>,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Args)]
pub struct TailArgs {
    /// Tenant slug
    #[arg(long, default_value = "default")]
    pub tenant: String,

    /// Agent ID to filter logs
    #[arg(long)]
    pub agent: Option<String>,

    /// API endpoint
    #[arg(long)]
    pub endpoint: Option<String>,

    /// Bearer token for remote API access
    #[arg(long)]
    pub token: Option<String>,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct LogEntry {
    #[serde(default)]
    id: String,
    #[serde(default)]
    agent_id: String,
    #[serde(default)]
    capability_id: String,
    #[serde(default)]
    success: bool,
    #[serde(default)]
    latency_ms: i64,
    #[serde(default)]
    error_message: Option<String>,
    #[serde(default)]
    created_at: String,
}

fn print_log_table(logs: &[LogEntry]) {
    println!(
        "{:<38} {:<38} {:<8} {:>8} {:<24} ERROR",
        "ID", "CAPABILITY", "OK", "MS", "TIME"
    );
    println!("{}", "-".repeat(120));
    for log in logs {
        let ok = if log.success { "yes" } else { "FAIL" };
        let err = log.error_message.as_deref().unwrap_or("");
        println!(
            "{:<38} {:<38} {:<8} {:>8} {:<24} {}",
            log.id, log.capability_id, ok, log.latency_ms, log.created_at, err
        );
    }
    println!("\n{} log(s)", logs.len());
}

pub async fn run_logs(args: LogsArgs) -> anyhow::Result<()> {
    let client = ApiClient::new(args.endpoint.clone(), args.token.clone());
    let tid = &args.tenant;

    let path = if let Some(ref aid) = args.agent {
        format!("/api/tenants/{tid}/agents/{aid}/logs?limit={}", args.limit)
    } else {
        format!("/api/tenants/{tid}/logs?limit={}", args.limit)
    };

    let logs: Vec<LogEntry> = client.get(&path).await.unwrap_or_default();

    if args.json {
        println!("{}", serde_json::to_string_pretty(&logs)?);
    } else if logs.is_empty() {
        println!("No capability invocation logs found.");
    } else {
        print_log_table(&logs);
    }

    Ok(())
}

pub async fn run_tail(args: TailArgs) -> anyhow::Result<()> {
    let client = ApiClient::new(args.endpoint.clone(), args.token.clone());
    let tid = &args.tenant;
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    println!("Tailing logs (Ctrl-C to stop)...\n");

    loop {
        let path = if let Some(ref aid) = args.agent {
            format!("/api/tenants/{tid}/agents/{aid}/logs?limit=20")
        } else {
            format!("/api/tenants/{tid}/logs?limit=20")
        };

        let logs: Vec<LogEntry> = client.get(&path).await.unwrap_or_default();

        for log in &logs {
            if seen.insert(log.id.clone()) {
                if args.json {
                    println!("{}", serde_json::to_string(&log)?);
                } else {
                    let ok = if log.success { "OK" } else { "FAIL" };
                    let err = log.error_message.as_deref().unwrap_or("");
                    println!(
                        "[{}] {} cap={} {} {}ms {}",
                        log.created_at, log.agent_id, log.capability_id, ok, log.latency_ms, err
                    );
                }
            }
        }

        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}
