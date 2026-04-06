use clap::Parser;

#[derive(Parser)]
#[command(name = "audit-extensions", about = "Audit Dream Server extension manifests")]
struct Args {
    #[arg(short, long)]
    dir: Option<String>,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    dream_scripts::audit_extensions::run(args.dir.as_deref())
}
