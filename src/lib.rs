pub mod cli;
pub mod doctor;
pub mod relay;
pub mod s3;
pub mod storage;
pub mod ticket;
pub mod transfer;
pub mod webdav;

use anyhow::Result;
use cli::{Cli, Command};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn install_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

pub async fn run_cli() -> Result<()> {
    install_crypto_provider();

    let cli = Cli::parse();
    match cli.command {
        Command::Send(args) => transfer::send(args).await?,
        Command::Web(args) => transfer::web(args).await?,
        Command::Webrtc(args) => transfer::webrtc(args).await?,
        Command::Tunnel(args) => transfer::tunnel(args).await?,
        Command::Recv(args) => transfer::recv(args).await?,
        Command::Relay(args) => relay::run(args).await?,
        Command::Doctor => doctor::run().await?,
        Command::Version => println!("{}", env!("CARGO_PKG_VERSION")),
    }

    Ok(())
}
