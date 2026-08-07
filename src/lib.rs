pub mod backend;
pub mod cli;
pub mod command;
pub mod discovery;
pub mod doctor;
pub mod json;
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
    let (json_mode, operation) = match &cli.command {
        Command::Send(args) => (args.json, "send"),
        Command::Recv(args) => (args.json, "recv"),
        Command::Discover(args) => (args.json, "discover"),
        _ => (false, "command"),
    };
    let result = match cli.command {
        Command::Send(args) => service::send(args).await,
        Command::Watch(args) => service::watch(args).await,
        Command::Queue(args) => service::queue(args).await,
        Command::Web(args) => service::web(args).await,
        Command::Dav(args) => service::dav(args).await,
        Command::Socks5(args) => service::socks5(args).await,
        Command::Webrtc(args) => service::webrtc(args).await,
        Command::Tunnel(args) => service::tunnel(args).await,
        Command::Recv(args) => service::recv(args).await,
        Command::Relay(args) => relay::run(args).await,
        Command::Discover(args) => service::discover(args).await,
        Command::Doctor(args) => doctor::run(args).await,
        Command::Version => {
            println!("{}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
    };
    if let Err(err) = result {
        if json_mode {
            json::error(operation, &format!("{err:#}"));
        }
        return Err(err);
    }
    Ok(())
}
