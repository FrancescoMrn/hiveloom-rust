use clap::Args;
use std::io::{self, BufRead, Write};

use super::client::ApiClient;

#[derive(Args)]
pub struct ChatArgs {
    /// Agent name or ID
    pub agent: String,

    /// Tenant slug (default: "default")
    #[arg(long, default_value_t = crate::cli::local::default_tenant())]
    pub tenant: String,

    /// API endpoint
    #[arg(long)]
    pub endpoint: Option<String>,

    /// Bearer token for remote API access
    #[arg(long)]
    pub token: Option<String>,
}

#[derive(serde::Deserialize)]
struct ChatResponse {
    response: String,
    conversation_id: String,
    #[serde(default)]
    capabilities_used: Vec<String>,
}

pub async fn run(args: ChatArgs) -> anyhow::Result<()> {
    let client = ApiClient::new(args.endpoint.clone(), args.token.clone());
    let tid = &args.tenant;
    let agent = &args.agent;

    eprintln!("Chatting with {} (Ctrl-C to exit)\n", agent);

    let mut conversation_id: Option<String> = None;
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    loop {
        eprint!("you: ");
        stdout.flush()?;

        let mut line = String::new();
        let bytes = stdin.lock().read_line(&mut line)?;
        if bytes == 0 {
            // EOF (Ctrl-D)
            break;
        }

        let message = line.trim();
        if message.is_empty() {
            continue;
        }
        if message == "/exit" {
            break;
        }

        let mut body = serde_json::json!({ "message": message });
        if let Some(ref cid) = conversation_id {
            body["conversation_id"] = serde_json::Value::String(cid.clone());
        }

        match client
            .post::<_, ChatResponse>(
                &format!("/api/tenants/{tid}/agents/{agent}/chat"),
                &body,
            )
            .await
        {
            Ok(resp) => {
                conversation_id = Some(resp.conversation_id);
                println!("{}: {}", agent, resp.response);
                if !resp.capabilities_used.is_empty() {
                    println!("  [capabilities: {}]", resp.capabilities_used.join(", "));
                }
                println!();
            }
            Err(e) => {
                eprintln!("  error: {}\n", e);
            }
        }
    }

    eprintln!("Chat ended.");
    Ok(())
}
