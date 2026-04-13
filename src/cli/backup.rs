use clap::{Args, Subcommand};
use serde::{Deserialize, Serialize};

use super::client::ApiClient;

#[derive(Args)]
pub struct BackupArgs {
    #[command(subcommand)]
    pub command: BackupCommand,

    /// API endpoint
    #[arg(long, global = true)]
    pub endpoint: Option<String>,

    /// Bearer token for remote API access
    #[arg(long, global = true)]
    pub token: Option<String>,

    /// Output as JSON
    #[arg(long, global = true)]
    pub json: bool,
}

#[derive(Subcommand)]
pub enum BackupCommand {
    /// Create a new backup
    Create {
        /// Tenant slug to back up (all if omitted)
        #[arg(long)]
        tenant: Option<String>,
        /// Output file path
        #[arg(long, default_value = "hiveloom-backup.tar.gz")]
        output: String,
    },
    /// List available backups
    List,
    /// Restore from a backup
    Restore {
        /// Backup file to restore from
        #[arg(long)]
        input: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct BackupInfo {
    #[serde(default)]
    id: String,
    #[serde(default)]
    filename: String,
    #[serde(default)]
    size_bytes: u64,
    #[serde(default)]
    created_at: String,
}

pub async fn run(args: BackupArgs) -> anyhow::Result<()> {
    let client = ApiClient::new(args.endpoint.clone(), args.token.clone());
    let json_out = args.json;

    match args.command {
        BackupCommand::Create { tenant, output } => {
            let body = serde_json::json!({
                "tenant": tenant,
                "output": output,
            });
            let result: serde_json::Value = client.post("/api/backups", &body).await?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("Backup created: {}", output);
            }
        }
        BackupCommand::List => {
            let backups: Vec<BackupInfo> = client.get("/api/backups").await.unwrap_or_default();
            if json_out {
                println!("{}", serde_json::to_string_pretty(&backups)?);
            } else if backups.is_empty() {
                println!("No backups found.");
            } else {
                println!(
                    "{:<38} {:<30} {:>12} {:<24}",
                    "ID", "FILENAME", "SIZE", "CREATED"
                );
                println!("{}", "-".repeat(110));
                for b in &backups {
                    println!(
                        "{:<38} {:<30} {:>12} {:<24}",
                        b.id, b.filename, b.size_bytes, b.created_at
                    );
                }
                println!("\n{} backup(s)", backups.len());
            }
        }
        BackupCommand::Restore { input } => {
            let body = serde_json::json!({ "input": input });
            let result: serde_json::Value = client.post("/api/backups/restore", &body).await?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("Restore initiated from: {}", input);
            }
        }
    }

    Ok(())
}
