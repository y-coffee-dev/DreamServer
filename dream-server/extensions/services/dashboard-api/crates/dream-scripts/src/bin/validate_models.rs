use clap::Parser;

#[derive(Parser)]
#[command(name = "validate-models", about = "Validate model backend configurations")]
struct Args {
    #[arg(short, long)]
    config: Option<String>,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    dream_scripts::validate_models::run(args.config.as_deref())
}
