pub mod agent;
pub mod apply;
pub mod auth;
pub mod backup;
pub mod capability;
pub mod credential;
pub mod health;
pub mod interactive;
pub mod logs;
pub mod mcp_identity;
pub mod serve;
pub mod tenant;
pub mod top;
pub mod upgrade;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "hiveloom", version, about = "Multi-tenant AI agent platform")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Start the Hiveloom service
    Serve(serve::ServeArgs),
    /// Manage agents
    Agent(agent::AgentArgs),
    /// Manage capabilities
    Capability(capability::CapabilityArgs),
    /// Manage credentials
    Credential(credential::CredentialArgs),
    /// Manage tenants
    Tenant(tenant::TenantArgs),
    /// Manage authentication tokens
    Auth(auth::AuthArgs),
    /// Manage MCP identities
    McpIdentity(mcp_identity::McpIdentityArgs),
    /// Apply manifests
    Apply(apply::ApplyArgs),
    /// Live status dashboard
    Top(top::TopArgs),
    /// View logs
    Logs(logs::LogsArgs),
    /// Stream logs
    Tail(logs::TailArgs),
    /// Check instance health
    Health(health::HealthArgs),
    /// Run diagnostics
    Doctor(health::DoctorArgs),
    /// Show service status
    Status(health::StatusArgs),
    /// Upgrade to latest release
    Upgrade(upgrade::UpgradeArgs),
    /// Rollback to previous release
    Rollback(upgrade::RollbackArgs),
    /// Show version
    Version,
    /// Manage backups
    Backup(backup::BackupArgs),
}

pub async fn dispatch(cli: Cli) -> anyhow::Result<()> {
    match cli.command {
        Command::Serve(args) => serve::run(args).await,
        Command::Agent(args) => agent::run(args).await,
        Command::Capability(args) => capability::run(args).await,
        Command::Credential(args) => credential::run(args).await,
        Command::Tenant(args) => tenant::run(args).await,
        Command::Auth(args) => auth::run(args).await,
        Command::McpIdentity(args) => mcp_identity::run(args).await,
        Command::Apply(args) => apply::run(args).await,
        Command::Top(args) => top::run(args).await,
        Command::Logs(args) => logs::run_logs(args).await,
        Command::Tail(args) => logs::run_tail(args).await,
        Command::Health(args) => health::run_health(args).await,
        Command::Doctor(args) => health::run_doctor(args).await,
        Command::Status(args) => health::run_status(args).await,
        Command::Upgrade(args) => upgrade::run_upgrade(args).await,
        Command::Rollback(args) => upgrade::run_rollback(args).await,
        Command::Version => {
            println!("hiveloom {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Command::Backup(args) => backup::run(args).await,
    }
}
