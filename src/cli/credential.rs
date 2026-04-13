use clap::{Args, Subcommand};

#[derive(Args)]
pub struct CredentialArgs {
    #[command(subcommand)]
    pub command: CredentialCommand,
}

#[derive(Subcommand)]
pub enum CredentialCommand {
    Set,
    List,
    Rotate { id: String },
    Remove { id: String },
}

pub async fn run(_args: CredentialArgs) -> anyhow::Result<()> {
    todo!()
}
