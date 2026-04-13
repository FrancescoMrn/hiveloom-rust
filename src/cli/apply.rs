use clap::Args;
use serde::{Deserialize, Serialize};

use super::client::ApiClient;

#[derive(Args)]
pub struct ApplyArgs {
    /// Path to manifest file (YAML or JSON)
    #[arg(long, short)]
    pub file: String,

    /// Tenant slug (default: "default")
    #[arg(long, default_value = "default")]
    pub tenant: String,

    /// API endpoint
    #[arg(long)]
    pub endpoint: Option<String>,

    /// Bearer token for remote API access
    #[arg(long)]
    pub token: Option<String>,

    /// Delete agents not present in the manifest
    #[arg(long)]
    pub prune: bool,

    /// Output as JSON
    #[arg(long)]
    pub json: bool,
}

// ── Manifest schema ─────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
struct Manifest {
    #[serde(default)]
    pub agents: Vec<AgentManifestEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AgentManifestEntry {
    pub name: String,
    #[serde(default = "default_model")]
    pub model: String,
    #[serde(default)]
    pub system_prompt: String,
    #[serde(default = "default_scope")]
    pub scope_mode: String,
    #[serde(default)]
    pub capabilities: Vec<CapabilityEntry>,
    #[serde(default)]
    pub bindings: Vec<BindingEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CapabilityEntry {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub endpoint_url: String,
    #[serde(default = "default_auth")]
    pub auth_type: String,
    #[serde(default)]
    pub credential_ref: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct BindingEntry {
    pub surface_type: String,
    pub surface_ref: String,
}

fn default_model() -> String {
    "claude-sonnet-4-20250514".to_string()
}
fn default_scope() -> String {
    "tenant_only".to_string()
}
fn default_auth() -> String {
    "none".to_string()
}

// ── Response types ──────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
struct AgentResponse {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub version: i64,
}

// ── Reconciliation summary ──────────────────────────────────────────

#[derive(Debug, Default, Serialize)]
struct ReconcileSummary {
    created: Vec<String>,
    updated: Vec<String>,
    pruned: Vec<String>,
    errors: Vec<String>,
}

pub async fn run(args: ApplyArgs) -> anyhow::Result<()> {
    let client = ApiClient::new(args.endpoint.clone(), args.token.clone());
    let tid = &args.tenant;

    // Read and parse manifest
    let raw = std::fs::read_to_string(&args.file)
        .map_err(|e| anyhow::anyhow!("cannot read '{}': {}", args.file, e))?;
    let manifest: Manifest = if args.file.ends_with(".json") {
        serde_json::from_str(&raw)?
    } else {
        serde_yaml::from_str(&raw)?
    };

    // Fetch existing agents
    let existing: Vec<AgentResponse> = client
        .get(&format!("/api/tenants/{tid}/agents"))
        .await
        .unwrap_or_default();
    let existing_names: std::collections::HashSet<String> =
        existing.iter().map(|a| a.name.clone()).collect();
    let existing_by_name: std::collections::HashMap<String, &AgentResponse> =
        existing.iter().map(|a| (a.name.clone(), a)).collect();

    let mut summary = ReconcileSummary::default();
    let mut manifest_names = std::collections::HashSet::new();

    for entry in &manifest.agents {
        manifest_names.insert(entry.name.clone());

        let body = serde_json::json!({
            "name": entry.name,
            "model_id": entry.model,
            "system_prompt": entry.system_prompt,
            "scope_mode": entry.scope_mode,
        });

        if existing_names.contains(&entry.name) {
            // Update
            let agent_id = &existing_by_name[&entry.name].id;
            match client
                .put::<_, AgentResponse>(
                    &format!("/api/tenants/{tid}/agents/{agent_id}"),
                    &body,
                )
                .await
            {
                Ok(a) => summary.updated.push(format!("{} (v{})", a.name, a.version)),
                Err(e) => summary.errors.push(format!("{}: {}", entry.name, e)),
            }
        } else {
            // Create
            match client
                .post::<_, AgentResponse>(
                    &format!("/api/tenants/{tid}/agents"),
                    &body,
                )
                .await
            {
                Ok(a) => summary.created.push(a.name),
                Err(e) => summary.errors.push(format!("{}: {}", entry.name, e)),
            }
        }
    }

    // Prune agents not in manifest
    if args.prune {
        for agent in &existing {
            if !manifest_names.contains(&agent.name) {
                match client
                    .delete(&format!("/api/tenants/{tid}/agents/{}", agent.id))
                    .await
                {
                    Ok(()) => summary.pruned.push(agent.name.clone()),
                    Err(e) => summary.errors.push(format!("prune {}: {}", agent.name, e)),
                }
            }
        }
    }

    // Print summary
    if args.json {
        println!("{}", serde_json::to_string_pretty(&summary)?);
    } else {
        println!("Reconciliation complete:");
        if !summary.created.is_empty() {
            println!("  Created: {}", summary.created.join(", "));
        }
        if !summary.updated.is_empty() {
            println!("  Updated: {}", summary.updated.join(", "));
        }
        if !summary.pruned.is_empty() {
            println!("  Pruned:  {}", summary.pruned.join(", "));
        }
        if !summary.errors.is_empty() {
            println!("  Errors:");
            for e in &summary.errors {
                println!("    - {e}");
            }
        }
        if summary.created.is_empty()
            && summary.updated.is_empty()
            && summary.pruned.is_empty()
            && summary.errors.is_empty()
        {
            println!("  No changes.");
        }
    }

    if !summary.errors.is_empty() {
        anyhow::bail!("{} error(s) during apply", summary.errors.len());
    }

    Ok(())
}
