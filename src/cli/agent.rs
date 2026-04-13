use clap::{Args, Subcommand};
use serde::{Deserialize, Serialize};

use super::client::ApiClient;

#[derive(Args)]
pub struct AgentArgs {
    #[command(subcommand)]
    pub command: AgentCommand,

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
pub enum AgentCommand {
    /// Create a new agent
    Create {
        /// Agent name
        #[arg(long)]
        name: String,
        /// LLM model identifier
        #[arg(long, default_value = "claude-sonnet-4-20250514")]
        model: String,
        /// System prompt text
        #[arg(long, default_value = "You are a helpful assistant.")]
        system_prompt: String,
        /// Scope mode (dual | tenant-only | user-only)
        #[arg(long, default_value = "dual")]
        scope_mode: String,
    },
    /// List agents in a tenant
    List,
    /// Show agent details
    Show { id: String },
    /// Edit an existing agent
    Edit {
        id: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        system_prompt: Option<String>,
    },
    /// Delete an agent
    Delete { id: String },
    /// List agent versions
    Versions { id: String },
    /// Rollback agent to a previous version
    Rollback {
        id: String,
        /// Target version number
        #[arg(long)]
        to_version: i64,
    },
    /// Export agent definition as YAML manifest
    Export { id: String },
    /// Trigger agent self-reflection (placeholder)
    Reflect { id: String },
    /// Bind agent to a chat surface
    Bind {
        id: String,
        /// Surface type (e.g. slack)
        #[arg(long)]
        surface: String,
        /// Surface reference (e.g. channel ID)
        #[arg(long)]
        channel: String,
    },
    /// View or update compaction configuration for an agent (T025)
    Compaction {
        /// Agent ID or name
        id: String,
        /// Compaction trigger threshold percentage (50-100)
        #[arg(long)]
        threshold: Option<i64>,
        /// Maximum summary fraction percentage (10-50)
        #[arg(long)]
        max_summary: Option<i64>,
        /// Number of recent turns to protect (1-20)
        #[arg(long)]
        protected_turns: Option<i64>,
        /// Show compaction indicator to end users
        #[arg(long)]
        show_indicator: Option<bool>,
        /// Reset to platform defaults (remove agent override)
        #[arg(long)]
        reset: bool,
    },
}

// ── API response types (mirrors server JSON) ────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct AgentResponse {
    pub id: String,
    #[serde(default)]
    pub tenant_id: String,
    pub name: String,
    #[serde(default)]
    pub system_prompt: String,
    #[serde(default)]
    pub model_id: String,
    #[serde(default)]
    pub scope_mode: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub version: i64,
    #[serde(default)]
    pub is_current: bool,
    #[serde(default)]
    pub created_at: String,
    /// T036: Compaction count for this agent
    #[serde(default)]
    pub compaction_count: u64,
    /// T036: Last compaction timestamp
    #[serde(default)]
    pub last_compaction_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CapabilityResponse {
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
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BindingResponse {
    pub id: String,
    pub surface_type: String,
    pub surface_ref: String,
}

// ── YAML manifest types for export/import ───────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct AgentManifest {
    pub name: String,
    pub model: String,
    pub system_prompt: String,
    pub scope_mode: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<CapabilityManifest>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bindings: Vec<BindingManifest>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CapabilityManifest {
    pub name: String,
    pub description: String,
    pub endpoint_url: String,
    pub auth_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_ref: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BindingManifest {
    pub surface_type: String,
    pub surface_ref: String,
}

// ── Dispatcher ──────────────────────────────────────────────────────

pub async fn run(args: AgentArgs) -> anyhow::Result<()> {
    let client = ApiClient::new(args.endpoint.clone(), args.token.clone());
    let tid = &args.tenant;
    let json_out = args.json;

    match args.command {
        AgentCommand::Create {
            name,
            model,
            system_prompt,
            scope_mode,
        } => {
            let body = serde_json::json!({
                "name": name,
                "model_id": model,
                "system_prompt": system_prompt,
                "scope_mode": scope_mode,
            });
            let agent: AgentResponse = client
                .post(&format!("/api/tenants/{tid}/agents"), &body)
                .await?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&agent)?);
            } else {
                println!("Created agent {} ({})", agent.name, agent.id);
            }
        }
        AgentCommand::List => {
            let agents: Vec<AgentResponse> =
                client.get(&format!("/api/tenants/{tid}/agents")).await?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&agents)?);
            } else {
                println!(
                    "{:<38} {:<24} {:<16} {:<10} {:>4}",
                    "ID", "NAME", "MODEL", "STATUS", "VER"
                );
                println!("{}", "-".repeat(94));
                for a in &agents {
                    println!(
                        "{:<38} {:<24} {:<16} {:<10} {:>4}",
                        a.id, a.name, a.model_id, a.status, a.version
                    );
                }
                println!("\n{} agent(s)", agents.len());
            }
        }
        AgentCommand::Show { id } => {
            let agent: AgentResponse = client
                .get(&format!("/api/tenants/{tid}/agents/{id}"))
                .await?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&agent)?);
            } else {
                print_agent_detail(&agent);
            }
        }
        AgentCommand::Edit {
            id,
            name,
            model,
            system_prompt,
        } => {
            // Fetch current agent to merge with provided fields
            let current: AgentResponse = client
                .get(&format!("/api/tenants/{tid}/agents/{id}"))
                .await?;
            let body = serde_json::json!({
                "name": name.unwrap_or(current.name),
                "model_id": model.unwrap_or(current.model_id),
                "system_prompt": system_prompt.unwrap_or(current.system_prompt),
            });
            let agent: AgentResponse = client
                .put(&format!("/api/tenants/{tid}/agents/{id}"), &body)
                .await?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&agent)?);
            } else {
                println!("Updated agent {} (v{})", agent.name, agent.version);
            }
        }
        AgentCommand::Delete { id } => {
            client
                .delete(&format!("/api/tenants/{tid}/agents/{id}"))
                .await?;
            println!("Deleted agent {id}");
        }
        AgentCommand::Versions { id } => {
            let versions: Vec<AgentResponse> = client
                .get(&format!("/api/tenants/{tid}/agents/{id}/versions"))
                .await?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&versions)?);
            } else {
                println!(
                    "{:>4}  {:<10} {:<24} {:<30}",
                    "VER", "CURRENT", "MODEL", "CREATED"
                );
                println!("{}", "-".repeat(72));
                for v in &versions {
                    let marker = if v.is_current { "*" } else { "" };
                    println!(
                        "{:>4}  {:<10} {:<24} {:<30}",
                        v.version, marker, v.model_id, v.created_at
                    );
                }
            }
        }
        AgentCommand::Rollback { id, to_version } => {
            let body = serde_json::json!({ "version": to_version });
            let agent: AgentResponse = client
                .post(&format!("/api/tenants/{tid}/agents/{id}/rollback"), &body)
                .await?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&agent)?);
            } else {
                println!(
                    "Rolled back agent {} to version {} (now v{})",
                    agent.name, to_version, agent.version
                );
            }
        }
        AgentCommand::Export { id } => {
            // T051: full export — agent + capabilities + bindings
            let agent: AgentResponse = client
                .get(&format!("/api/tenants/{tid}/agents/{id}"))
                .await?;
            let caps: Vec<CapabilityResponse> = client
                .get(&format!("/api/tenants/{tid}/agents/{id}/capabilities"))
                .await
                .unwrap_or_default();
            let bindings: Vec<BindingResponse> = client
                .get(&format!("/api/tenants/{tid}/agents/{id}/bindings"))
                .await
                .unwrap_or_default();

            let manifest = AgentManifest {
                name: agent.name,
                model: agent.model_id,
                system_prompt: agent.system_prompt,
                scope_mode: agent.scope_mode,
                capabilities: caps
                    .into_iter()
                    .map(|c| CapabilityManifest {
                        name: c.name,
                        description: c.description,
                        endpoint_url: c.endpoint_url,
                        auth_type: c.auth_type,
                        credential_ref: c.credential_ref,
                    })
                    .collect(),
                bindings: bindings
                    .into_iter()
                    .map(|b| BindingManifest {
                        surface_type: b.surface_type,
                        surface_ref: b.surface_ref,
                    })
                    .collect(),
            };
            println!("{}", serde_yaml::to_string(&manifest)?);
        }
        AgentCommand::Reflect { id } => {
            let body = serde_json::json!({});
            let _: serde_json::Value = client
                .post(&format!("/api/tenants/{tid}/agents/{id}/reflect"), &body)
                .await?;
            println!("Reflection triggered for agent {id}");
        }
        AgentCommand::Bind {
            id,
            surface,
            channel,
        } => {
            let body = serde_json::json!({
                "surface_type": surface,
                "surface_ref": channel,
            });
            let binding: BindingResponse = client
                .post(&format!("/api/tenants/{tid}/agents/{id}/bindings"), &body)
                .await?;
            if json_out {
                println!("{}", serde_json::to_string_pretty(&binding)?);
            } else {
                println!(
                    "Bound agent {id} to {} (ref: {})",
                    binding.surface_type, binding.surface_ref
                );
            }
        }
        AgentCommand::Compaction {
            id,
            threshold,
            max_summary,
            protected_turns,
            show_indicator,
            reset,
        } => {
            let is_set_mode = threshold.is_some()
                || max_summary.is_some()
                || protected_turns.is_some()
                || show_indicator.is_some()
                || reset;

            if is_set_mode {
                // PATCH mode
                let body = serde_json::json!({
                    "threshold_pct": threshold,
                    "max_summary_fraction_pct": max_summary,
                    "protected_turn_count": protected_turns,
                    "show_indicator": show_indicator,
                    "reset": reset,
                });
                let result: serde_json::Value = client
                    .patch(
                        &format!("/api/tenants/{tid}/agents/{id}/compaction-config"),
                        &body,
                    )
                    .await?;
                if json_out {
                    println!("{}", serde_json::to_string_pretty(&result)?);
                } else {
                    println!("Updated compaction config for agent \"{}\":", id);
                    print_compaction_config(&result);
                }
            } else {
                // GET mode
                let result: serde_json::Value = client
                    .get(&format!("/api/tenants/{tid}/agents/{id}/compaction-config"))
                    .await?;
                if json_out {
                    println!("{}", serde_json::to_string_pretty(&result)?);
                } else {
                    println!("Compaction config for agent \"{}\":", id);
                    print_compaction_config(&result);
                }
            }
        }
    }

    Ok(())
}

fn print_compaction_config(config: &serde_json::Value) {
    let source = config
        .get("source")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let threshold = config
        .get("threshold_pct")
        .and_then(|v| v.as_i64())
        .unwrap_or(80);
    let max_summary = config
        .get("max_summary_fraction_pct")
        .and_then(|v| v.as_i64())
        .unwrap_or(30);
    let protected = config
        .get("protected_turn_count")
        .and_then(|v| v.as_i64())
        .unwrap_or(4);
    let indicator = config
        .get("show_indicator")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    println!("  Threshold:        {}% ({})", threshold, source);
    println!("  Max summary:      {}% ({})", max_summary, source);
    println!("  Protected turns:  {} ({})", protected, source);
    println!("  Show indicator:   {} ({})", indicator, source);
    println!("  Source:           {}", source);
}

fn print_agent_detail(a: &AgentResponse) {
    println!("Agent:        {}", a.name);
    println!("ID:           {}", a.id);
    println!("Tenant:       {}", a.tenant_id);
    println!("Model:        {}", a.model_id);
    println!("Scope mode:   {}", a.scope_mode);
    println!("Status:       {}", a.status);
    println!("Version:      {}", a.version);
    println!("Current:      {}", a.is_current);
    println!("Created:      {}", a.created_at);
    println!("System prompt:");
    for line in a.system_prompt.lines() {
        println!("  {line}");
    }
    // T036: Compaction info
    println!("Compaction:");
    println!("  Count:          {}", a.compaction_count);
    println!(
        "  Last compacted: {}",
        a.last_compaction_at.as_deref().unwrap_or("never")
    );
}
