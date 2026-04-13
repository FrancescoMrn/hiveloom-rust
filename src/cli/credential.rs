use clap::{Args, Subcommand};
use serde::{Deserialize, Serialize};
use std::io::Read as _;

use super::client::ApiClient;

#[derive(Args)]
pub struct CredentialArgs {
    #[command(subcommand)]
    pub command: CredentialCommand,

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
pub enum CredentialCommand {
    /// Store a credential (reads value from env var, file, or stdin -- never a CLI flag)
    Set {
        /// Credential name
        name: String,
        /// Credential kind (static | oauth2)
        #[arg(long, default_value = "static")]
        kind: String,
        /// Read secret value from this environment variable
        #[arg(long, group = "source")]
        from_env: Option<String>,
        /// Read secret value from this file path
        #[arg(long, group = "source")]
        from_file: Option<String>,
        // If neither --from-env nor --from-file is given, read from stdin.
    },
    /// List credential names (never shows values)
    List,
    /// Rotate a credential's secret value
    Rotate {
        /// Credential name
        name: String,
        /// Read new secret from this environment variable
        #[arg(long, group = "source")]
        from_env: Option<String>,
        /// Read new secret from this file path
        #[arg(long, group = "source")]
        from_file: Option<String>,
    },
    /// Remove a credential
    Remove {
        /// Credential name
        name: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct CredentialInfo {
    pub id: String,
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub rotated_at: Option<String>,
}

/// Read the secret value from the specified source (env var, file, or stdin).
/// IMPORTANT: The secret is never accepted as a visible CLI flag value (FR-048).
fn read_secret(from_env: &Option<String>, from_file: &Option<String>) -> anyhow::Result<String> {
    if let Some(var) = from_env {
        std::env::var(var).map_err(|_| anyhow::anyhow!("environment variable '{}' is not set", var))
    } else if let Some(path) = from_file {
        std::fs::read_to_string(path)
            .map(|s| s.trim_end().to_string())
            .map_err(|e| anyhow::anyhow!("failed to read file '{}': {}", path, e))
    } else {
        // Read from stdin
        if atty::is(atty::Stream::Stdin) {
            eprintln!("Enter secret value (then press Ctrl-D):");
        }
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        Ok(buf.trim_end().to_string())
    }
}

pub async fn run(args: CredentialArgs) -> anyhow::Result<()> {
    let client = ApiClient::new(args.endpoint.clone(), args.token.clone());
    let tid = &args.tenant;
    let json_out = args.json;

    match args.command {
        CredentialCommand::Set {
            name,
            kind,
            from_env,
            from_file,
        } => {
            let secret = read_secret(&from_env, &from_file)?;
            let body = serde_json::json!({
                "name": name,
                "kind": kind,
                "value": secret,
            });
            let cred: CredentialInfo = client
                .post(&format!("/api/tenants/{tid}/credentials"), &body)
                .await?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&cred)?);
            } else {
                println!("Stored credential '{}' ({})", cred.name, cred.id);
            }
        }
        CredentialCommand::List => {
            let creds: Vec<CredentialInfo> = client
                .get(&format!("/api/tenants/{tid}/credentials"))
                .await?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&creds)?);
            } else {
                println!(
                    "{:<38} {:<24} {:<10} {:<30}",
                    "ID", "NAME", "KIND", "UPDATED"
                );
                println!("{}", "-".repeat(104));
                for c in &creds {
                    println!(
                        "{:<38} {:<24} {:<10} {:<30}",
                        c.id, c.name, c.kind, c.updated_at
                    );
                }
                println!("\n{} credential(s)", creds.len());
            }
        }
        CredentialCommand::Rotate {
            name,
            from_env,
            from_file,
        } => {
            let secret = read_secret(&from_env, &from_file)?;
            let body = serde_json::json!({ "value": secret });
            let cred: CredentialInfo = client
                .post(
                    &format!("/api/tenants/{tid}/credentials/{name}/rotate"),
                    &body,
                )
                .await?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&cred)?);
            } else {
                println!("Rotated credential '{}'", cred.name);
            }
        }
        CredentialCommand::Remove { name } => {
            client
                .delete(&format!("/api/tenants/{tid}/credentials/{name}"))
                .await?;
            println!("Removed credential '{name}'");
        }
    }

    Ok(())
}
