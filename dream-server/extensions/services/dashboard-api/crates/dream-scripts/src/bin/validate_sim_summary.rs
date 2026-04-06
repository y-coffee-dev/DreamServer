use clap::Parser;

#[derive(Parser)]
#[command(name = "validate-sim-summary", about = "Validate installer simulation summary")]
struct Args {
    #[arg(short, long)]
    file: String,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    dream_scripts::validate_sim_summary::run(&args.file)
}
