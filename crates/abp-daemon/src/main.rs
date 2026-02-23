use anyhow::Result;
use clap::Parser;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(name = "abp-daemon", version, about = "Agent Backplane daemon (stub)")]
struct Args {
    /// Bind address.
    #[arg(long, default_value = "127.0.0.1:8088")]
    bind: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new("abp=info"))
        .try_init();

    let args = Args::parse();

    eprintln!("abp-daemon is a stub in v0.1");
    eprintln!("bind requested: {}", args.bind);
    eprintln!("\nNext: implement an HTTP control-plane API that exposes /run, /capabilities, /receipts.");

    Ok(())
}
