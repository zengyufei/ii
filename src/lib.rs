pub mod backend;
pub mod cli;
pub mod command;
pub mod doctor;
pub mod relay;
pub mod s3;
pub mod service;
pub mod storage;
pub mod ticket;
pub mod transfer;
pub mod transport;
pub mod web;
pub mod webdav;

use anyhow::Result;
use command::{Cli, Command};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub fn install_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

pub async fn run_cli() -> Result<()> {
    install_crypto_provider();

    let cli = Cli::parse();
    match cli.command {
        Command::Send(args) => service::send(args).await?,
        Command::Web(args) => service::web(args).await?,
        Command::Webrtc(args) => service::webrtc(args).await?,
        Command::Tunnel(args) => service::tunnel(args).await?,
        Command::Recv(args) => service::recv(args).await?,
        Command::Relay(args) => relay::run(args).await?,
        Command::Doctor => doctor::run().await?,
        Command::Version => println!("{}", env!("CARGO_PKG_VERSION")),
    }

    Ok(())
}
