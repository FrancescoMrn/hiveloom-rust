use clap::Args;

#[derive(Args)]
pub struct UpgradeArgs {}

#[derive(Args)]
pub struct RollbackArgs {}

pub async fn run_upgrade(_args: UpgradeArgs) -> anyhow::Result<()> {
    todo!()
}

pub async fn run_rollback(_args: RollbackArgs) -> anyhow::Result<()> {
    todo!()
}
