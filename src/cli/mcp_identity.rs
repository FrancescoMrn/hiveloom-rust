use clap::{Args, Subcommand};

#[derive(Args)]
pub struct McpIdentityArgs {
    #[command(subcommand)]
    pub command: McpIdentityCommand,
}

#[derive(Subcommand)]
pub enum McpIdentityCommand {
    Create,
    List,
    Show { id: String },
    Map { id: String },
    Unmap { id: String },
    Revoke { id: String },
    ReissueSetupCode { id: String },
}

pub async fn run(_args: McpIdentityArgs) -> anyhow::Result<()> {
    todo!()
}
