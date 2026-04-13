use clap::Args;

#[derive(Args)]
pub struct HealthArgs {}

#[derive(Args)]
pub struct DoctorArgs {}

#[derive(Args)]
pub struct StatusArgs {}

pub async fn run_health(_args: HealthArgs) -> anyhow::Result<()> {
    todo!()
}

pub async fn run_doctor(_args: DoctorArgs) -> anyhow::Result<()> {
    todo!()
}

pub async fn run_status(_args: StatusArgs) -> anyhow::Result<()> {
    todo!()
}
