use clap::{Args, Subcommand};

#[derive(Args)]
pub struct BackupArgs {
    #[command(subcommand)]
    pub command: BackupCommand,
}

#[derive(Subcommand)]
pub enum BackupCommand {
    Create,
    List,
    Restore { id: String },
}

pub async fn run(_args: BackupArgs) -> anyhow::Result<()> {
    todo!()
}
