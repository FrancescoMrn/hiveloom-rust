use clap::Parser;
use hiveloom::cli;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let args: Vec<String> = std::env::args().collect();

    // Bare `hiveloom` with no subcommand → interactive mode (FR-049)
    if args.len() == 1 {
        if atty::is(atty::Stream::Stdin) {
            return cli::interactive::run().await;
        } else {
            anyhow::bail!(
                "Interactive mode requires a controlling terminal.\n\
                 Run `hiveloom serve` to start the service, or \
                 `hiveloom --help` for available subcommands."
            );
        }
    }

    let cli = cli::Cli::parse();
    cli::dispatch(cli).await
}
