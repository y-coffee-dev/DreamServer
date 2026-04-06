use clap::Parser;

#[derive(Parser)]
#[command(name = "assign-gpus", about = "Assign GPUs to Dream Server services")]
struct Args {
    #[arg(short, long)]
    topology: Option<String>,
    #[arg(long)]
    dry_run: bool,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    dream_scripts::assign_gpus::run(args.topology.as_deref(), args.dry_run)
}
