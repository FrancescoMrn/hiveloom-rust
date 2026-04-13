use clap::{Args, Subcommand};
use serde::{Deserialize, Serialize};

use super::client::ApiClient;

#[derive(Args)]
pub struct TenantArgs {
    #[command(subcommand)]
    pub command: TenantCommand,

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
pub enum TenantCommand {
    /// Create a new tenant
    Create {
        /// Tenant display name
        #[arg(long)]
        name: String,
        /// URL-friendly slug
        #[arg(long)]
        slug: String,
        /// IANA timezone (e.g. America/New_York)
        #[arg(long, default_value = "UTC")]
        timezone: String,
    },
    /// List all tenants
    List,
    /// Show tenant details
    Show {
        /// Tenant ID or slug
        id: String,
    },
    /// Disable a tenant
    Disable {
        /// Tenant ID
        id: String,
    },
    /// Enable a disabled tenant
    Enable {
        /// Tenant ID
        id: String,
    },
    /// Delete a tenant (soft-delete)
    Delete {
        /// Tenant ID
        id: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct TenantResponse {
    pub id: String,
    pub name: String,
    pub slug: String,
    #[serde(default)]
    pub timezone: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

pub async fn run(args: TenantArgs) -> anyhow::Result<()> {
    let client = ApiClient::new(args.endpoint.clone(), args.token.clone());
    let json_out = args.json;

    match args.command {
        TenantCommand::Create {
            name,
            slug,
            timezone,
        } => {
            let body = serde_json::json!({
                "name": name,
                "slug": slug,
                "timezone": timezone,
            });
            let tenant: TenantResponse = client.post("/api/tenants", &body).await?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&tenant)?);
            } else {
                println!("Created tenant '{}' ({})", tenant.name, tenant.id);
            }
        }
        TenantCommand::List => {
            let tenants: Vec<TenantResponse> = client.get("/api/tenants").await?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&tenants)?);
            } else {
                println!(
                    "{:<38} {:<20} {:<16} {:<20} {:<10}",
                    "ID", "NAME", "SLUG", "TIMEZONE", "STATUS"
                );
                println!("{}", "-".repeat(106));
                for t in &tenants {
                    println!(
                        "{:<38} {:<20} {:<16} {:<20} {:<10}",
                        t.id, t.name, t.slug, t.timezone, t.status
                    );
                }
                println!("\n{} tenant(s)", tenants.len());
            }
        }
        TenantCommand::Show { id } => {
            let tenant: TenantResponse = client.get(&format!("/api/tenants/{id}")).await?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&tenant)?);
            } else {
                println!("Name:     {}", tenant.name);
                println!("ID:       {}", tenant.id);
                println!("Slug:     {}", tenant.slug);
                println!("Timezone: {}", tenant.timezone);
                println!("Status:   {}", tenant.status);
                println!("Created:  {}", tenant.created_at);
                println!("Updated:  {}", tenant.updated_at);
            }
        }
        TenantCommand::Disable { id } => {
            let body = serde_json::json!({ "status": "disabled" });
            let tenant: TenantResponse = client
                .put(&format!("/api/tenants/{id}/status"), &body)
                .await?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&tenant)?);
            } else {
                println!("Disabled tenant '{}' ({})", tenant.name, tenant.id);
            }
        }
        TenantCommand::Enable { id } => {
            let body = serde_json::json!({ "status": "active" });
            let tenant: TenantResponse = client
                .put(&format!("/api/tenants/{id}/status"), &body)
                .await?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&tenant)?);
            } else {
                println!("Enabled tenant '{}' ({})", tenant.name, tenant.id);
            }
        }
        TenantCommand::Delete { id } => {
            client.delete(&format!("/api/tenants/{id}")).await?;
            println!("Deleted tenant {id}");
        }
    }

    Ok(())
}
