use clap::Parser;

#[derive(Parser)]
#[command(name = "healthcheck", about = "Check health of all Dream Server services")]
struct Args {
    #[arg(short, long, default_value = "text")]
    format: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    dream_scripts::healthcheck::run(&args.format).await
}
