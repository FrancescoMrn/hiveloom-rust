use clap::{Args, Subcommand};

use super::client::ApiClient;

#[derive(Args)]
pub struct McpIdentityArgs {
    #[command(subcommand)]
    pub command: McpIdentityCommand,

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
pub enum McpIdentityCommand {
    /// Create a new MCP identity
    Create {
        /// Tenant ID
        #[arg(long)]
        tenant: String,
        /// Name for the MCP identity
        #[arg(long)]
        name: String,
        /// Agent slug or UUID to bind this identity to (optional)
        #[arg(long)]
        agent: Option<String>,
    },
    /// List MCP identities for a tenant
    List {
        /// Tenant ID
        #[arg(long)]
        tenant: String,
        /// Filter by agent slug or UUID
        #[arg(long)]
        agent: Option<String>,
    },
    /// Show details of an MCP identity
    Show {
        /// MCP identity ID
        id: String,
        /// Tenant ID
        #[arg(long)]
        tenant: String,
    },
    /// Map an MCP identity to a person
    Map {
        /// MCP identity ID
        id: String,
        /// Tenant ID
        #[arg(long)]
        tenant: String,
        /// Person ID to map to
        #[arg(long)]
        person_id: String,
    },
    /// Unmap an MCP identity from its person
    Unmap {
        /// MCP identity ID
        id: String,
        /// Tenant ID
        #[arg(long)]
        tenant: String,
    },
    /// Revoke an MCP identity
    Revoke {
        /// MCP identity ID
        id: String,
        /// Tenant ID
        #[arg(long)]
        tenant: String,
    },
    /// Reissue a setup code for an MCP identity
    ReissueSetupCode {
        /// MCP identity ID
        id: String,
        /// Tenant ID
        #[arg(long)]
        tenant: String,
    },
}

pub async fn run(args: McpIdentityArgs) -> anyhow::Result<()> {
    let client = ApiClient::new(args.endpoint.clone(), args.token.clone());

    match args.command {
        McpIdentityCommand::Create {
            tenant,
            name,
            agent,
        } => {
            let mut body = serde_json::json!({ "name": name });
            if let Some(ref agent_id) = agent {
                body["agent_id"] = serde_json::Value::String(agent_id.clone());
            }
            let result: serde_json::Value = client
                .post(&format!("/api/tenants/{}/mcp-identities", tenant), &body)
                .await?;
            let identity_id = result.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();

            // Immediately issue a setup code so users have everything they need
            let setup = client
                .post::<_, serde_json::Value>(
                    &format!(
                        "/api/tenants/{}/mcp-identities/{}/reissue-setup-code",
                        tenant, identity_id
                    ),
                    &serde_json::json!({}),
                )
                .await
                .ok();

            if args.json {
                let mut combined = result.clone();
                if let Some(setup_val) = setup.as_ref() {
                    if let Some(code) = setup_val.get("setup_code") {
                        combined["setup_code"] = code.clone();
                    }
                    if let Some(exp) = setup_val.get("expires_at") {
                        combined["setup_code_expires_at"] = exp.clone();
                    }
                }
                println!("{}", serde_json::to_string_pretty(&combined)?);
            } else {
                println!("Created MCP identity '{}' ({})", name, identity_id);
                if let Some(setup_val) = setup {
                    if let Some(code) = setup_val.get("setup_code").and_then(|v| v.as_str()) {
                        let endpoint = crate::cli::local::default_endpoint();
                        let endpoint = endpoint.trim_end_matches('/');
                        println!();
                        println!("  Setup code:  {}", code);
                        if let Some(ref a) = agent {
                            println!("  MCP URL:     {}/mcp/{}/{}", endpoint, tenant, a);
                        } else {
                            println!("  MCP URL:     {}/mcp/{}/<agent-slug>", endpoint, tenant);
                        }
                        println!();
                        println!("  Add the URL to your MCP client (Claude Desktop, Cursor, etc.).");
                        println!("  Enter the setup code in the browser when prompted.");
                    }
                }
            }
        }
        McpIdentityCommand::List { tenant, agent } => {
            let url = match &agent {
                Some(a) => format!("/api/tenants/{}/mcp-identities?agent={}", tenant, a),
                None => format!("/api/tenants/{}/mcp-identities", tenant),
            };
            let result: serde_json::Value = client.get(&url).await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        McpIdentityCommand::Show { id, tenant } => {
            let result: serde_json::Value = client
                .get(&format!("/api/tenants/{}/mcp-identities/{}", tenant, id))
                .await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        McpIdentityCommand::Map {
            id,
            tenant,
            person_id,
        } => {
            let body = serde_json::json!({ "person_id": person_id });
            let result: serde_json::Value = client
                .post(
                    &format!("/api/tenants/{}/mcp-identities/{}/map", tenant, id),
                    &body,
                )
                .await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        McpIdentityCommand::Unmap { id, tenant } => {
            let body = serde_json::json!({});
            let result: serde_json::Value = client
                .post(
                    &format!("/api/tenants/{}/mcp-identities/{}/unmap", tenant, id),
                    &body,
                )
                .await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        McpIdentityCommand::Revoke { id, tenant } => {
            let body = serde_json::json!({});
            let result: serde_json::Value = client
                .post(
                    &format!("/api/tenants/{}/mcp-identities/{}/revoke", tenant, id),
                    &body,
                )
                .await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        McpIdentityCommand::ReissueSetupCode { id, tenant } => {
            let body = serde_json::json!({});
            let result: serde_json::Value = client
                .post(
                    &format!(
                        "/api/tenants/{}/mcp-identities/{}/reissue-setup-code",
                        tenant, id
                    ),
                    &body,
                )
                .await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
    }

    Ok(())
}
