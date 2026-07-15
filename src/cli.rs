use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "ii", version, about = "ii file transfer")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Send(SendArgs),
    Recv(RecvArgs),
    Relay(RelayArgs),
    Doctor,
    Version,
}

#[derive(Debug, Args, Clone)]
pub struct SendArgs {
    pub path: Option<PathBuf>,

    #[arg(long)]
    pub name: Option<String>,

    #[arg(short = 't')]
    pub keep_alive: bool,

    #[arg(long, conflicts_with_all = ["relay", "no_relay"])]
    pub local: bool,

    #[arg(long, value_name = "url", conflicts_with_all = ["local", "no_relay"])]
    pub relay: Option<iroh::RelayUrl>,

    #[arg(long, conflicts_with_all = ["local", "relay"])]
    pub no_relay: bool,
}

#[derive(Debug, Args, Clone)]
pub struct RecvArgs {
    pub ticket: String,

    #[arg(short = 'o', value_name = "dir")]
    pub out_dir: Option<PathBuf>,

    #[arg(long, conflicts_with = "resume")]
    pub stdout: bool,

    #[arg(long)]
    pub overwrite: bool,

    #[arg(long, conflicts_with = "stdout")]
    pub resume: bool,

    #[arg(long)]
    pub local: bool,

    #[arg(long)]
    pub trace: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn recv_accepts_trace() {
        let cli = Cli::parse_from(["ii", "recv", "ii1abc", "--trace"]);
        match cli.command {
            Command::Recv(args) => assert!(args.trace),
            _ => panic!("expected recv command"),
        }
    }

    #[test]
    fn send_accepts_keep_alive() {
        let cli = Cli::parse_from(["ii", "send", "file.txt", "-t"]);
        match cli.command {
            Command::Send(args) => assert!(args.keep_alive),
            _ => panic!("expected send command"),
        }
    }
}

#[derive(Debug, Args, Clone)]
pub struct RelayArgs {
    #[arg(long)]
    pub dev: bool,

    #[arg(long, short = 'c')]
    pub config: Option<PathBuf>,

    #[arg(long = "http", short = 'H')]
    pub http: Option<u16>,

    #[arg(long = "https", short = 'S')]
    pub https: Option<u16>,

    #[arg(long = "quic", short = 'Q')]
    pub quic: Option<u16>,

    #[arg(long = "metrics", short = 'M')]
    pub metrics: Option<u16>,
}
