use clap::{Args, Subcommand};

#[derive(Args)]
pub struct CapabilityArgs {
    #[command(subcommand)]
    pub command: CapabilityCommand,
}

#[derive(Subcommand)]
pub enum CapabilityCommand {
    Add,
    List,
    Show { id: String },
    Edit { id: String },
    Remove { id: String },
}

pub async fn run(_args: CapabilityArgs) -> anyhow::Result<()> {
    todo!()
}
