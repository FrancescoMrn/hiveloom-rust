use clap::{Args, Subcommand};
use serde::{Deserialize, Serialize};

use super::client::ApiClient;

#[derive(Args)]
pub struct CapabilityArgs {
    #[command(subcommand)]
    pub command: CapabilityCommand,

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
pub enum CapabilityCommand {
    /// Add a capability to an agent
    Add {
        /// Agent ID
        agent: String,
        /// Capability name
        #[arg(long)]
        name: String,
        /// Description
        #[arg(long)]
        description: String,
        /// Endpoint URL the capability calls
        #[arg(long)]
        cap_endpoint: String,
        /// Auth type (none | api_key | oauth)
        #[arg(long, default_value = "none")]
        auth_type: String,
        /// Credential reference name
        #[arg(long)]
        credential_ref: Option<String>,
    },
    /// List capabilities for an agent
    List {
        /// Agent ID
        agent: String,
    },
    /// Show capability details
    Show {
        /// Agent ID
        agent: String,
        /// Capability name
        name: String,
    },
    /// Edit a capability
    Edit {
        /// Agent ID
        agent: String,
        /// Capability name
        name: String,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        cap_endpoint: Option<String>,
        #[arg(long)]
        auth_type: Option<String>,
        #[arg(long)]
        credential_ref: Option<String>,
    },
    /// Remove a capability from an agent
    Remove {
        /// Agent ID
        agent: String,
        /// Capability name
        name: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct CapabilityResponse {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub endpoint_url: String,
    #[serde(default)]
    pub auth_type: String,
    #[serde(default)]
    pub credential_ref: Option<String>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

pub async fn run(args: CapabilityArgs) -> anyhow::Result<()> {
    let client = ApiClient::new(args.endpoint.clone(), args.token.clone());
    let tid = &args.tenant;
    let json_out = args.json;

    match args.command {
        CapabilityCommand::Add {
            agent,
            name,
            description,
            cap_endpoint,
            auth_type,
            credential_ref,
        } => {
            let body = serde_json::json!({
                "name": name,
                "description": description,
                "endpoint_url": cap_endpoint,
                "auth_type": auth_type,
                "credential_ref": credential_ref,
            });
            let cap: CapabilityResponse = client
                .post(
                    &format!("/api/tenants/{tid}/agents/{agent}/capabilities"),
                    &body,
                )
                .await?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&cap)?);
            } else {
                println!("Added capability '{}' ({})", cap.name, cap.id);
            }
        }
        CapabilityCommand::List { agent } => {
            let caps: Vec<CapabilityResponse> = client
                .get(&format!("/api/tenants/{tid}/agents/{agent}/capabilities"))
                .await?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&caps)?);
            } else {
                println!(
                    "{:<38} {:<20} {:<12} {:<40}",
                    "ID", "NAME", "AUTH", "ENDPOINT"
                );
                println!("{}", "-".repeat(112));
                for c in &caps {
                    println!(
                        "{:<38} {:<20} {:<12} {:<40}",
                        c.id, c.name, c.auth_type, c.endpoint_url
                    );
                }
                println!("\n{} capability(ies)", caps.len());
            }
        }
        CapabilityCommand::Show { agent, name } => {
            let cap: CapabilityResponse = client
                .get(&format!(
                    "/api/tenants/{tid}/agents/{agent}/capabilities/{name}"
                ))
                .await?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&cap)?);
            } else {
                println!("Name:           {}", cap.name);
                println!("ID:             {}", cap.id);
                println!("Description:    {}", cap.description);
                println!("Endpoint:       {}", cap.endpoint_url);
                println!("Auth type:      {}", cap.auth_type);
                println!(
                    "Credential ref: {}",
                    cap.credential_ref.as_deref().unwrap_or("-")
                );
                println!("Created:        {}", cap.created_at);
                println!("Updated:        {}", cap.updated_at);
            }
        }
        CapabilityCommand::Edit {
            agent,
            name,
            description,
            cap_endpoint,
            auth_type,
            credential_ref,
        } => {
            let body = serde_json::json!({
                "description": description,
                "endpoint_url": cap_endpoint,
                "auth_type": auth_type,
                "credential_ref": credential_ref,
            });
            let cap: CapabilityResponse = client
                .put(
                    &format!("/api/tenants/{tid}/agents/{agent}/capabilities/{name}"),
                    &body,
                )
                .await?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&cap)?);
            } else {
                println!("Updated capability '{}'", cap.name);
            }
        }
        CapabilityCommand::Remove { agent, name } => {
            client
                .delete(&format!(
                    "/api/tenants/{tid}/agents/{agent}/capabilities/{name}"
                ))
                .await?;
            println!("Removed capability '{name}' from agent {agent}");
        }
    }

    Ok(())
}
