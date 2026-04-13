use clap::{Args, Subcommand};

#[derive(Args)]
pub struct TenantArgs {
    #[command(subcommand)]
    pub command: TenantCommand,
}

#[derive(Subcommand)]
pub enum TenantCommand {
    Create,
    List,
    Show { id: String },
    Disable { id: String },
    Enable { id: String },
    Delete { id: String },
}

pub async fn run(_args: TenantArgs) -> anyhow::Result<()> {
    todo!()
}
