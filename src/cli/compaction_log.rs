//! `hiveloom compaction-log` CLI subcommand (T024).
//!
//! Display recent compaction events for one or all agents.

use clap::Args;
use serde::Deserialize;

use super::client::ApiClient;

#[derive(Args)]
pub struct CompactionLogArgs {
    /// Filter by agent (ID or name)
    #[arg(long)]
    pub agent: Option<String>,

    /// Filter by tenant (default: "default")
    #[arg(long, default_value_t = crate::cli::local::default_tenant())]
    pub tenant: String,

    /// Show events from the last N hours/days (e.g. "1h", "6h", "24h", "7d", "30d")
    #[arg(long, default_value = "24h")]
    pub since: String,

    /// Maximum number of events to show
    #[arg(long, default_value = "50")]
    pub limit: u64,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,

    /// API endpoint
    #[arg(long)]
    pub endpoint: Option<String>,

    /// Bearer token for remote API access
    #[arg(long)]
    pub token: Option<String>,
}

#[derive(Debug, Deserialize, serde::Serialize)]
struct CompactionEventResponse {
    #[serde(default)]
    id: String,
    #[serde(default)]
    timestamp: String,
    #[serde(default)]
    agent_id: String,
    #[serde(default)]
    conversation_id: String,
    #[serde(default)]
    tokens_before: i64,
    #[serde(default)]
    tokens_after: i64,
    #[serde(default)]
    strategy: String,
    #[serde(default)]
    fallback_used: bool,
    #[serde(default)]
    summary_token_count: Option<i64>,
    #[serde(default)]
    error_message: Option<String>,
}

pub async fn run(args: CompactionLogArgs) -> anyhow::Result<()> {
    let client = ApiClient::new(args.endpoint.clone(), args.token.clone());
    let tid = &args.tenant;

    // Build query params
    let mut url = format!("/api/tenants/{tid}/compaction-events?limit={}", args.limit);

    if let Some(ref agent) = args.agent {
        url.push_str(&format!("&agent_id={}", agent));
    }

    // Parse --since into an ISO timestamp
    let since_timestamp = parse_since_duration(&args.since)?;
    url.push_str(&format!("&since={}", since_timestamp));

    let events: Vec<CompactionEventResponse> = client.get(&url).await.unwrap_or_default();

    if args.json {
        println!("{}", serde_json::to_string_pretty(&events)?);
    } else {
        println!(
            "{:<22} {:<16} {:<16} {:>8} {:>8} {:<16} {:<8}",
            "TIMESTAMP", "AGENT", "CONVERSATION", "BEFORE", "AFTER", "STRATEGY", "FALLBACK"
        );
        println!("{}", "-".repeat(100));
        for ev in &events {
            let agent_short = truncate_id(&ev.agent_id, 12);
            let conv_short = truncate_id(&ev.conversation_id, 12);
            let ts_short = ev.timestamp.get(..19).unwrap_or(&ev.timestamp);
            let fallback = if ev.fallback_used { "yes" } else { "no" };
            println!(
                "{:<22} {:<16} {:<16} {:>8} {:>8} {:<16} {:<8}",
                ts_short,
                agent_short,
                conv_short,
                ev.tokens_before,
                ev.tokens_after,
                ev.strategy,
                fallback
            );
        }
        println!("\n{} event(s)", events.len());
    }

    Ok(())
}

fn parse_since_duration(since: &str) -> anyhow::Result<String> {
    let now = chrono::Utc::now();
    let duration = if since.ends_with('h') {
        let hours: i64 = since.trim_end_matches('h').parse()?;
        chrono::Duration::hours(hours)
    } else if since.ends_with('d') {
        let days: i64 = since.trim_end_matches('d').parse()?;
        chrono::Duration::days(days)
    } else {
        // Default: treat as hours
        let hours: i64 = since.parse().unwrap_or(24);
        chrono::Duration::hours(hours)
    };
    let cutoff = now - duration;
    Ok(cutoff.to_rfc3339())
}

fn truncate_id(id: &str, max_len: usize) -> String {
    if id.len() <= max_len {
        id.to_string()
    } else {
        format!("{}...", &id[..max_len.saturating_sub(3)])
    }
}
