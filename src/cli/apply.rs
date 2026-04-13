use clap::Args;

#[derive(Args)]
pub struct ApplyArgs {
    /// Path to manifest file
    pub file: String,
}

pub async fn run(_args: ApplyArgs) -> anyhow::Result<()> {
    todo!()
}
