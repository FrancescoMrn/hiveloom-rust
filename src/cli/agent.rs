use clap::{Args, Subcommand};

#[derive(Args)]
pub struct AgentArgs {
    #[command(subcommand)]
    pub command: AgentCommand,
}

#[derive(Subcommand)]
pub enum AgentCommand {
    Create,
    List,
    Show { id: String },
    Edit { id: String },
    Delete { id: String },
    Versions { id: String },
    Rollback { id: String },
    Export { id: String },
    Reflect { id: String },
}

pub async fn run(_args: AgentArgs) -> anyhow::Result<()> {
    todo!()
}
