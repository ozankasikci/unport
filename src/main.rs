use clap::{Parser, Subcommand};
use unport_cli::{client, daemon, logger, tls};

#[derive(Parser)]
#[command(name = "unport")]
#[command(about = "Local development port manager with automatic domain routing")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Manage the unport daemon
    Daemon {
        #[command(subcommand)]
        action: DaemonAction,
    },
    /// Start the app in current directory and register with daemon
    Start,
    /// Stop a running service by domain
    Stop {
        /// Domain name to stop
        domain: String,
    },
    /// List all registered services
    List,
    /// Add unport CA to system trust store for HTTPS support
    TrustCa {
        /// Remove CA from trust store instead of adding
        #[arg(long)]
        remove: bool,
    },
    /// Delete generated TLS certificates (forces regeneration on next daemon start)
    CleanCerts,
    /// Regenerate TLS certificate with SANs for all registered domains
    RegenCert,
}

#[derive(Subcommand)]
enum DaemonAction {
    /// Start the daemon
    Start {
        /// Run daemon in background (detached)
        #[arg(short = 'd', long = "detach")]
        detach: bool,
        /// Enable HTTPS on port 443
        #[arg(long)]
        https: bool,
    },
    /// Stop the daemon
    Stop,
    /// Show daemon status
    Status,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    logger::init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Daemon { action } => match action {
            DaemonAction::Start { detach, https } => daemon::run(detach, https).await,
            DaemonAction::Stop => client::stop_daemon().await,
            DaemonAction::Status => client::daemon_status().await,
        },
        Commands::Start => client::start().await,
        Commands::Stop { domain } => client::stop_service(&domain).await,
        Commands::List => client::list().await,
        Commands::TrustCa { remove } => client::trust_ca(remove).await,
        Commands::CleanCerts => tls::clean_certs(),
        Commands::RegenCert => client::regen_cert().await,
    }
}
