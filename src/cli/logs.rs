use clap::Args;

#[derive(Args)]
pub struct LogsArgs {}

#[derive(Args)]
pub struct TailArgs {}

pub async fn run_logs(_args: LogsArgs) -> anyhow::Result<()> {
    todo!()
}

pub async fn run_tail(_args: TailArgs) -> anyhow::Result<()> {
    todo!()
}
