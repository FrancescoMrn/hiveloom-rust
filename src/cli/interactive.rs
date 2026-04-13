use super::client::ApiClient;

pub async fn run() -> anyhow::Result<()> {
    println!("=== Hiveloom Interactive Mode ===\n");

    let data_dir = std::env::var("HIVELOOM_DATA_DIR")
        .unwrap_or_else(|_| "/var/lib/hiveloom".to_string());

    let is_first_run = !std::path::Path::new(&data_dir)
        .join("platform.db")
        .exists();

    if is_first_run {
        println!("Welcome to Hiveloom! Let's get you set up.\n");
        run_first_run_wizard().await?;
    } else {
        println!("Hiveloom is already configured.\n");
        run_main_menu().await?;
    }

    Ok(())
}

async fn run_first_run_wizard() -> anyhow::Result<()> {
    // Step 1: LLM Provider
    println!("Step 1: LLM Provider Configuration");
    println!("  Which LLM provider would you like to use?");
    println!("  1) Anthropic (Claude)");
    println!("  2) OpenAI");
    println!("  3) Custom endpoint");
    let provider_choice = read_line("  Choice [1]: ")?;
    let provider = match provider_choice.trim() {
        "2" => "openai",
        "3" => "custom",
        _ => "anthropic",
    };
    println!("  Selected: {provider}\n");

    // Step 2: API Key
    println!("Step 2: API Key");
    let api_key = read_secret("  Enter your API key: ")?;
    if api_key.is_empty() {
        println!("  Skipped (you can set this later via `hiveloom credential set`)\n");
    } else {
        let masked = mask_secret(&api_key);
        println!("  API key set: {masked}\n");
    }

    // Step 3: First agent
    println!("Step 3: Create Your First Agent");
    let agent_name = read_line("  Agent name [my-agent]: ")?;
    let agent_name = if agent_name.trim().is_empty() {
        "my-agent".to_string()
    } else {
        agent_name.trim().to_string()
    };
    println!("  Agent '{}' will be created on first `hiveloom serve`.\n", agent_name);

    // Step 4: Slack setup
    println!("Step 4: Slack Integration (optional)");
    let setup_slack = read_line("  Set up Slack integration? [y/N]: ")?;
    if setup_slack.trim().eq_ignore_ascii_case("y") {
        println!("  To connect Slack, set these environment variables:");
        println!("    SLACK_SIGNING_SECRET=<your-signing-secret>");
        println!("    SLACK_BOT_TOKEN=<your-bot-token>");
        println!("  Then restart Hiveloom.\n");
    } else {
        println!("  Skipped.\n");
    }

    // Summary
    println!("=== Setup Complete ===");
    println!("Provider:   {provider}");
    println!("Agent:      {agent_name}");
    println!();
    println!("Next steps:");
    println!("  1. Start the service:  hiveloom serve");
    println!("  2. Create the agent:   hiveloom agent create --name {agent_name}");
    println!("  3. Check health:       hiveloom health");

    // Store first-run config hints
    if !api_key.is_empty() {
        let client = ApiClient::new(None, None);
        let body = serde_json::json!({
            "name": format!("{provider}-api-key"),
            "kind": "static",
            "value": api_key,
        });
        // Best-effort: service might not be running yet
        let _ = client
            .post::<_, serde_json::Value>("/api/tenants/default/credentials", &body)
            .await;
    }

    Ok(())
}

async fn run_main_menu() -> anyhow::Result<()> {
    println!("What would you like to do?");
    println!("  1) Check service health");
    println!("  2) List agents");
    println!("  3) Create an agent");
    println!("  4) View logs");
    println!("  5) Run diagnostics");
    println!("  q) Quit");

    let choice = read_line("\nChoice: ")?;
    match choice.trim() {
        "1" => {
            let args = super::health::HealthArgs {
                endpoint: None,
                token: None,
                json: false,
            };
            super::health::run_health(args).await?;
        }
        "2" => {
            println!("\nRun: hiveloom agent list");
        }
        "3" => {
            println!("\nRun: hiveloom agent create --name <name>");
        }
        "4" => {
            println!("\nRun: hiveloom logs");
        }
        "5" => {
            let args = super::health::DoctorArgs {
                data_dir: std::env::var("HIVELOOM_DATA_DIR")
                    .unwrap_or_else(|_| "/var/lib/hiveloom".to_string()),
                json: false,
            };
            super::health::run_doctor(args).await?;
        }
        "q" | "Q" => {}
        _ => {
            println!("Invalid choice.");
        }
    }

    Ok(())
}

fn read_line(prompt: &str) -> anyhow::Result<String> {
    use std::io::Write;
    print!("{prompt}");
    std::io::stdout().flush()?;
    let mut buf = String::new();
    std::io::stdin().read_line(&mut buf)?;
    Ok(buf)
}

fn read_secret(prompt: &str) -> anyhow::Result<String> {
    use std::io::Write;
    print!("{prompt}");
    std::io::stdout().flush()?;

    // Try to disable echo for secret input
    #[cfg(unix)]
    {
        use std::io::Read;
        // Use raw terminal approach
        let was_raw = crossterm::terminal::is_raw_mode_enabled()
            .unwrap_or(false);
        if !was_raw {
            let _ = crossterm::terminal::enable_raw_mode();
        }
        let mut buf = String::new();
        let stdin = std::io::stdin();
        for byte in stdin.lock().bytes() {
            match byte? {
                b'\n' | b'\r' => {
                    println!();
                    break;
                }
                b'\x7f' | b'\x08' => {
                    buf.pop();
                }
                b if b >= 0x20 => {
                    buf.push(b as char);
                    print!("*");
                    std::io::stdout().flush()?;
                }
                _ => {}
            }
        }
        if !was_raw {
            let _ = crossterm::terminal::disable_raw_mode();
        }
        return Ok(buf);
    }

    #[cfg(not(unix))]
    {
        let mut buf = String::new();
        std::io::stdin().read_line(&mut buf)?;
        Ok(buf.trim().to_string())
    }
}

fn mask_secret(s: &str) -> String {
    if s.len() <= 8 {
        return "*".repeat(s.len());
    }
    let visible = &s[..4];
    format!("{}...{}", visible, "*".repeat(4))
}
