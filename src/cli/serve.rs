use clap::Args;

#[derive(Args)]
pub struct ServeArgs {
    /// Address to bind to
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,
    /// Port to listen on
    #[arg(long, default_value = "3000")]
    pub port: u16,
    /// Path to data directory
    #[arg(long, env = "HIVELOOM_DATA_DIR", default_value_t = crate::cli::local::default_data_dir())]
    pub data_dir: String,
    /// Disable the background scheduler loop
    #[arg(long, default_value_t = false)]
    pub no_scheduler: bool,
}

pub async fn run(args: ServeArgs) -> anyhow::Result<()> {
    tracing::info!(host = %args.host, port = args.port, "Starting Hiveloom service");
    crate::cli::local::write_local_config(
        std::path::Path::new(&args.data_dir),
        &args.host,
        args.port,
    )?;

    let app_state = crate::server::AppState::new(&args.data_dir).await?;

    // FR-032/FR-033: Auto-provision default tenant on first run
    app_state.platform_store.ensure_default_tenant()?;

    if !args.no_scheduler {
        let scheduler_data_dir = args.data_dir.clone();
        tokio::spawn(async move {
            let scheduler = crate::engine::JobScheduler::new(&scheduler_data_dir);
            if let Err(e) = scheduler.run().await {
                tracing::error!(error = %e, "Scheduler stopped");
            }
        });
    }

    let router = crate::server::create_router(app_state);

    let addr = format!("{}:{}", args.host, args.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!(%addr, "Listening");

    axum::serve(
        listener,
        router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await?;
    Ok(())
}
