mod cli;
mod doctor;
mod relay;
mod storage;
mod ticket;
mod transfer;

use anyhow::Result;
use cli::{Cli, Command};

#[tokio::main]
async fn main() -> Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();

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
