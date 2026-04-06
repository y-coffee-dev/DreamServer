//! dream-scripts: CLI entry point for Dream Server operational scripts.
//!
//! Run with: `dream-scripts <subcommand>`
//! Individual binaries are also available: healthcheck, assign-gpus, etc.

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "dream-scripts", version, about = "Dream Server operational scripts")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run a health check against all configured services
    Healthcheck {
        /// Output format: text or json
        #[arg(short, long, default_value = "text")]
        format: String,
    },
    /// Assign GPUs to services based on topology
    AssignGpus {
        /// Path to GPU topology JSON
        #[arg(short, long)]
        topology: Option<String>,
        /// Dry run — show assignment without applying
        #[arg(long)]
        dry_run: bool,
    },
    /// Audit extension manifests for consistency
    AuditExtensions {
        /// Path to extensions directory
        #[arg(short, long)]
        dir: Option<String>,
    },
    /// Validate a simulation summary file
    ValidateSimSummary {
        /// Path to simulation summary JSON
        #[arg(short, long)]
        file: String,
    },
    /// Validate model configuration
    ValidateModels {
        /// Path to backend config JSON
        #[arg(short, long)]
        config: Option<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Healthcheck { format } => {
            dream_scripts::healthcheck::run(&format).await
        }
        Commands::AssignGpus { topology, dry_run } => {
            dream_scripts::assign_gpus::run(topology.as_deref(), dry_run)
        }
        Commands::AuditExtensions { dir } => {
            dream_scripts::audit_extensions::run(dir.as_deref())
        }
        Commands::ValidateSimSummary { file } => {
            dream_scripts::validate_sim_summary::run(&file)
        }
        Commands::ValidateModels { config } => {
            dream_scripts::validate_models::run(config.as_deref())
        }
    }
}
