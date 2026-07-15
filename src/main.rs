mod cli;
mod doctor;
mod relay;
mod ticket;
mod transfer;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Command};

#[tokio::main]
async fn main() -> Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .without_time()
        .init();

    let cli = Cli::parse();
    match cli.command {
        Command::Send(args) => transfer::send(args).await?,
        Command::Recv(args) => transfer::recv(args).await?,
        Command::Relay(args) => relay::run(args).await?,
        Command::Doctor => doctor::run().await?,
        Command::Version => {
            println!("{}", env!("CARGO_PKG_VERSION"));
        }
    }

    Ok(())
}
