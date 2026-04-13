use clap::{Args, Subcommand};

#[derive(Args)]
pub struct AuthArgs {
    #[command(subcommand)]
    pub command: AuthCommand,
}

#[derive(Subcommand)]
pub enum AuthCommand {
    TokenCreate,
    TokenList,
    TokenRevoke { id: String },
}

pub async fn run(_args: AuthArgs) -> anyhow::Result<()> {
    todo!()
}
