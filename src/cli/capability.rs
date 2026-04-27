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
        /// Agent ID or name
        agent: String,
        /// Capability name
        #[arg(long)]
        name: String,
        /// Description
        #[arg(long)]
        description: String,
        /// Endpoint URL the capability calls (required for HTTP capabilities, omit for --from-file)
        #[arg(long)]
        cap_endpoint: Option<String>,
        /// Auth type (none | api_key | oauth | markdown)
        #[arg(long, default_value = "none")]
        auth_type: String,
        /// Credential reference name
        #[arg(long)]
        credential_ref: Option<String>,
        /// Load capability as a markdown skill from a file
        #[arg(long)]
        from_file: Option<String>,
    },
    /// List capabilities for an agent
    List {
        /// Agent ID or name
        agent: String,
    },
    /// Show capability details
    Show {
        /// Agent ID or name
        agent: String,
        /// Capability ID or name
        capability: String,
    },
    /// Edit a capability
    Edit {
        /// Agent ID or name
        agent: String,
        /// Capability ID or name
        capability: String,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        cap_endpoint: Option<String>,
        #[arg(long)]
        auth_type: Option<String>,
        #[arg(long)]
        credential_ref: Option<String>,
        /// Replace markdown skill content from a file
        #[arg(long)]
        from_file: Option<String>,
    },
    /// Remove a capability from an agent
    Remove {
        /// Agent ID or name
        agent: String,
        /// Capability ID or name
        capability: String,
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
    pub input_schema: Option<String>,
    #[serde(default)]
    pub output_schema: Option<String>,
    #[serde(default)]
    pub instruction_content: Option<String>,
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
            from_file,
        } => {
            let body = if let Some(ref path) = from_file {
                let content = std::fs::read_to_string(path)
                    .map_err(|e| anyhow::anyhow!("Failed to read file '{}': {}", path, e))?;
                let size = content.len();
                let body = serde_json::json!({
                    "name": name,
                    "description": description,
                    "auth_type": "markdown",
                    "instruction_content": content,
                });
                if !json_out {
                    eprintln!("Reading {} ({} bytes)...", path, size);
                }
                body
            } else {
                let endpoint = cap_endpoint
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("--cap-endpoint is required for HTTP capabilities (or use --from-file for markdown skills)"))?;
                serde_json::json!({
                    "name": name,
                    "description": description,
                    "endpoint_url": endpoint,
                    "auth_type": auth_type,
                    "credential_ref": credential_ref,
                })
            };
            let cap: CapabilityResponse = client
                .post(
                    &format!("/api/tenants/{tid}/agents/{agent}/capabilities"),
                    &body,
                )
                .await?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&cap)?);
            } else if from_file.is_some() {
                println!("Added markdown skill '{}' ({})", cap.name, cap.id);
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
        CapabilityCommand::Show { agent, capability } => {
            let cap: CapabilityResponse = client
                .get(&format!(
                    "/api/tenants/{tid}/agents/{agent}/capabilities/{capability}"
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
            capability,
            description,
            cap_endpoint,
            auth_type,
            credential_ref,
            from_file,
        } => {
            let current: CapabilityResponse = client
                .get(&format!(
                    "/api/tenants/{tid}/agents/{agent}/capabilities/{capability}"
                ))
                .await?;

            let instruction_content = if let Some(ref path) = from_file {
                let content = std::fs::read_to_string(path)
                    .map_err(|e| anyhow::anyhow!("Failed to read file '{}': {}", path, e))?;
                if !json_out {
                    eprintln!("Reading {} ({} bytes)...", path, content.len());
                }
                Some(content)
            } else {
                current.instruction_content
            };

            let body = serde_json::json!({
                "name": current.name,
                "description": description.unwrap_or(current.description),
                "endpoint_url": cap_endpoint.unwrap_or(current.endpoint_url),
                "auth_type": auth_type.unwrap_or(current.auth_type),
                "credential_ref": credential_ref.or(current.credential_ref),
                "input_schema": current.input_schema,
                "output_schema": current.output_schema,
                "instruction_content": instruction_content,
            });
            let cap: CapabilityResponse = client
                .put(
                    &format!("/api/tenants/{tid}/agents/{agent}/capabilities/{capability}"),
                    &body,
                )
                .await?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&cap)?);
            } else {
                println!("Updated capability '{}'", cap.name);
            }
        }
        CapabilityCommand::Remove { agent, capability } => {
            client
                .delete(&format!(
                    "/api/tenants/{tid}/agents/{agent}/capabilities/{capability}"
                ))
                .await?;
            println!("Removed capability '{capability}' from agent {agent}");
        }
    }

    Ok(())
}
