use clap::{Args, Subcommand};
use serde::{Deserialize, Serialize};

use super::client::ApiClient;

#[derive(Args)]
pub struct AuthArgs {
    #[command(subcommand)]
    pub command: AuthCommand,

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
pub enum AuthCommand {
    /// Create a new auth token
    TokenCreate {
        /// Token scope (e.g., "platform:admin")
        #[arg(long, default_value = "platform:admin")]
        scope: String,
        /// Expiration (ISO 8601 datetime, optional)
        #[arg(long)]
        expires_at: Option<String>,
    },
    /// List auth tokens
    TokenList,
    /// Revoke an auth token
    TokenRevoke {
        /// Token ID to revoke
        id: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct TokenResponse {
    #[serde(default)]
    id: String,
    #[serde(default)]
    scope: String,
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    expires_at: Option<String>,
    #[serde(default)]
    revoked_at: Option<String>,
    /// The plaintext token is only returned on creation
    #[serde(default)]
    token: Option<String>,
}

pub async fn run(args: AuthArgs) -> anyhow::Result<()> {
    let client = ApiClient::new(args.endpoint.clone(), args.token.clone());
    let json_out = args.json;

    match args.command {
        AuthCommand::TokenCreate { scope, expires_at } => {
            let body = serde_json::json!({
                "scope": scope,
                "expires_at": expires_at,
            });
            let resp: TokenResponse = client.post("/api/auth/tokens", &body).await?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                println!("Token created: {}", resp.id);
                if let Some(ref tok) = resp.token {
                    println!("Token value (save this, it will not be shown again):");
                    println!("  {tok}");
                }
                println!("Scope: {}", resp.scope);
            }
        }
        AuthCommand::TokenList => {
            let tokens: Vec<TokenResponse> = client.get("/api/auth/tokens").await?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&tokens)?);
            } else if tokens.is_empty() {
                println!("No auth tokens found.");
            } else {
                println!(
                    "{:<38} {:<20} {:<24} {:<24} REVOKED",
                    "ID", "SCOPE", "CREATED", "EXPIRES"
                );
                println!("{}", "-".repeat(120));
                for t in &tokens {
                    let expires = t.expires_at.as_deref().unwrap_or("never");
                    let revoked = t.revoked_at.as_deref().unwrap_or("-");
                    println!(
                        "{:<38} {:<20} {:<24} {:<24} {}",
                        t.id, t.scope, t.created_at, expires, revoked
                    );
                }
                println!("\n{} token(s)", tokens.len());
            }
        }
        AuthCommand::TokenRevoke { id } => {
            client.delete(&format!("/api/auth/tokens/{id}")).await?;
            println!("Token {id} revoked.");
        }
    }

    Ok(())
}
