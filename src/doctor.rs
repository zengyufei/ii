use anyhow::Result;
use std::net::{Ipv4Addr, SocketAddr, TcpListener, UdpSocket};

use crate::{relay, storage};

pub async fn run() -> Result<()> {
    let config_path = relay::default_config_path()?;
    let s3_config_path = storage::default_config_path()?;
    println!("ii version: {}", env!("CARGO_PKG_VERSION"));
    println!("platform: {}", std::env::consts::OS);
    println!("config: {}", config_path.display());
    println!(
        "relay config exists: {}",
        if config_path.exists() { "yes" } else { "no" }
    );
    println!("s3 config: {}", s3_config_path.display());
    println!(
        "s3 config exists: {}",
        if s3_config_path.exists() { "yes" } else { "no" }
    );
    report_s3_config(&s3_config_path);
    report_webdav_config(&s3_config_path);

    check_tcp("http", 80);
    check_tcp("https", 443);
    check_tcp("metrics", 9090);
    check_udp("quic", 7842);
    Ok(())
}

fn report_s3_config(path: &std::path::Path) {
    if !path.exists() {
        return;
    }
    match storage::load_config(path) {
        Ok(config) => {
            let profile = config
                .storage
                .profile
                .filter(|profile| config.storage.s3.contains_key(profile))
                .unwrap_or_else(|| "cloudflare".to_string());
            println!("s3 profile: {profile}");
            match config.storage.s3.get(&profile) {
                Some(s3) => {
                    println!("s3 provider: {}", s3.provider);
                    println!("s3 bucket: {}", s3.bucket);
                    println!("s3 endpoint: {}", s3.endpoint);
                    println!(
                        "s3 credentials: {}",
                        if s3.access_key_id.is_empty() || s3.secret_access_key.is_empty() {
                            "missing"
                        } else {
                            "configured"
                        }
                    );
                }
                None => println!("s3 profile configured but missing profile block"),
            }
        }
        Err(err) => println!("s3 config parse failed: {err:#}"),
    }
}

fn report_webdav_config(path: &std::path::Path) {
    if !path.exists() {
        return;
    }
    match storage::load_config(path) {
        Ok(config) => {
            let profile = config
                .storage
                .profile
                .filter(|profile| config.storage.webdav.contains_key(profile))
                .unwrap_or_else(|| "default".to_string());
            println!("webdav profile: {profile}");
            match config.storage.webdav.get(&profile) {
                Some(webdav) => {
                    println!("webdav url: {}", webdav.url);
                    println!("webdav remote dir: {}", webdav.remote_dir);
                    println!("webdav auth: {:?}", webdav.auth);
                    println!(
                        "webdav credentials: {}",
                        if webdav.username.is_empty() || webdav.password.is_empty() {
                            "missing"
                        } else {
                            "configured"
                        }
                    );
                }
                None if config.storage.webdav.is_empty() => {
                    println!("webdav profile: not configured")
                }
                None => println!("webdav profile configured but missing profile block"),
            }
        }
        Err(err) => println!("webdav config parse failed: {err:#}"),
    }
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
