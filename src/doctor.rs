use anyhow::Result;
use std::net::{Ipv4Addr, SocketAddr, TcpListener, UdpSocket};

use crate::relay;

pub async fn run() -> Result<()> {
    let config_path = relay::default_config_path()?;
    println!("ii version: {}", env!("CARGO_PKG_VERSION"));
    println!("platform: {}", std::env::consts::OS);
    println!("config: {}", config_path.display());
    println!(
        "relay config exists: {}",
        if config_path.exists() { "yes" } else { "no" }
    );

    check_tcp("http", 80);
    check_tcp("https", 443);
    check_tcp("metrics", 9090);
    check_udp("quic", 7842);
    Ok(())
}

fn check_tcp(label: &str, port: u16) {
    let addr = SocketAddr::from((Ipv4Addr::UNSPECIFIED, port));
    match TcpListener::bind(addr) {
        Ok(listener) => {
            drop(listener);
            println!("{label}: bind ok on {addr}");
        }
        Err(err) => {
            println!("{label}: bind failed on {addr}: {err}");
        }
    }
}

fn check_udp(label: &str, port: u16) {
    let addr = SocketAddr::from((Ipv4Addr::UNSPECIFIED, port));
    match UdpSocket::bind(addr) {
        Ok(sock) => {
            drop(sock);
            println!("{label}: bind ok on {addr}");
        }
        Err(err) => {
            println!("{label}: bind failed on {addr}: {err}");
        }
    }
}
