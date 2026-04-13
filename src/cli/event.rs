use clap::{Args, Subcommand};
use serde::{Deserialize, Serialize};

use super::client::ApiClient;

#[derive(Args)]
pub struct EventArgs {
    #[command(subcommand)]
    pub command: EventCommand,

    /// Tenant slug (default: "default")
    #[arg(long, default_value = "default", global = true)]
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
pub enum EventCommand {
    /// Create an event subscription for an agent
    Subscribe {
        /// Agent ID
        agent: String,
        /// Event type to subscribe to
        #[arg(long)]
        event_type: String,
        /// Source filter (optional)
        #[arg(long)]
        source_filter: Option<String>,
        /// Auth token for webhook verification
        #[arg(long)]
        auth_token: String,
    },
    /// List event subscriptions for an agent
    List {
        /// Agent ID
        agent: String,
    },
    /// Show event subscription details
    Show {
        /// Agent ID
        agent: String,
        /// Subscription ID
        subscription: String,
    },
    /// Disable an event subscription
    Disable {
        /// Agent ID
        agent: String,
        /// Subscription ID
        subscription: String,
    },
    /// Enable an event subscription
    Enable {
        /// Agent ID
        agent: String,
        /// Subscription ID
        subscription: String,
    },
    /// Delete an event subscription
    Delete {
        /// Agent ID
        agent: String,
        /// Subscription ID
        subscription: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct EventSubscriptionResponse {
    pub id: String,
    #[serde(default)]
    pub tenant_id: String,
    #[serde(default)]
    pub agent_id: String,
    #[serde(default)]
    pub event_type: String,
    pub source_filter: Option<String>,
    #[serde(default)]
    pub auth_token_hash: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub created_at: String,
}

pub async fn run(args: EventArgs) -> anyhow::Result<()> {
    let client = ApiClient::new(args.endpoint.clone(), args.token.clone());
    let tid = &args.tenant;
    let json_out = args.json;

    match args.command {
        EventCommand::Subscribe {
            agent,
            event_type,
            source_filter,
            auth_token,
        } => {
            let body = serde_json::json!({
                "event_type": event_type,
                "source_filter": source_filter,
                "auth_token": auth_token,
            });
            let sub: EventSubscriptionResponse = client
                .post(
                    &format!("/api/tenants/{tid}/agents/{agent}/event-subscriptions"),
                    &body,
                )
                .await?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&sub)?);
            } else {
                println!(
                    "Created event subscription {} (event_type: {}, status: {})",
                    sub.id, sub.event_type, sub.status
                );
            }
        }
        EventCommand::List { agent } => {
            let subs: Vec<EventSubscriptionResponse> = client
                .get(&format!(
                    "/api/tenants/{tid}/agents/{agent}/event-subscriptions"
                ))
                .await?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&subs)?);
            } else {
                println!(
                    "{:<38} {:<20} {:<20} {:<10}",
                    "ID", "EVENT TYPE", "SOURCE FILTER", "STATUS"
                );
                println!("{}", "-".repeat(90));
                for s in &subs {
                    println!(
                        "{:<38} {:<20} {:<20} {:<10}",
                        s.id,
                        s.event_type,
                        s.source_filter.as_deref().unwrap_or("-"),
                        s.status,
                    );
                }
                println!("\n{} subscription(s)", subs.len());
            }
        }
        EventCommand::Show { agent, subscription } => {
            let s: EventSubscriptionResponse = client
                .get(&format!(
                    "/api/tenants/{tid}/agents/{agent}/event-subscriptions/{subscription}"
                ))
                .await?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&s)?);
            } else {
                println!("Subscription ID: {}", s.id);
                println!("Agent:           {}", s.agent_id);
                println!("Tenant:          {}", s.tenant_id);
                println!("Event type:      {}", s.event_type);
                println!("Source filter:   {}", s.source_filter.as_deref().unwrap_or("-"));
                println!("Status:          {}", s.status);
                println!("Created:         {}", s.created_at);
            }
        }
        EventCommand::Disable { agent, subscription } => {
            let s: EventSubscriptionResponse = client
                .post(
                    &format!(
                        "/api/tenants/{tid}/agents/{agent}/event-subscriptions/{subscription}/disable"
                    ),
                    &serde_json::json!({}),
                )
                .await?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&s)?);
            } else {
                println!("Disabled event subscription {}", s.id);
            }
        }
        EventCommand::Enable { agent, subscription } => {
            let s: EventSubscriptionResponse = client
                .post(
                    &format!(
                        "/api/tenants/{tid}/agents/{agent}/event-subscriptions/{subscription}/enable"
                    ),
                    &serde_json::json!({}),
                )
                .await?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&s)?);
            } else {
                println!("Enabled event subscription {}", s.id);
            }
        }
        EventCommand::Delete { agent, subscription } => {
            client
                .delete(&format!(
                    "/api/tenants/{tid}/agents/{agent}/event-subscriptions/{subscription}"
                ))
                .await?;
            println!("Deleted event subscription {subscription}");
        }
    }

    Ok(())
}
