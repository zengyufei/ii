use anyhow::Result;

use crate::{relay, storage};

pub async fn run() -> Result<()> {
    let config_path = relay::default_config_path()?;
    let s3_config_path = storage::default_config_path()?;
    println!("ii version: {}", env!("CARGO_PKG_VERSION"));
    println!("platform: {}", std::env::consts::OS);
    println!("relay state: {}", config_path.display());
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
    report_ftp_config(&s3_config_path);
    report_sftp_config(&s3_config_path);

    println!("relay modes: self-signed or manual TLS, always relay-only");
    println!("relay start: ii relay --public https://PUBLIC_HOST[:PORT]");
    Ok(())
}

fn report_s3_config(path: &std::path::Path) {
    if !path.exists() {
        return;
    }
    match storage::load_config(path) {
        Ok(config) => {
            let profile = if config.storage.s3.contains_key("default") {
                "default".to_string()
            } else if config.storage.s3.contains_key("cloudflare") {
                "cloudflare".to_string()
            } else {
                "default".to_string()
            };
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
            let profile = "default".to_string();
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

fn report_ftp_config(path: &std::path::Path) {
    if !path.exists() {
        return;
    }
    match storage::load_config(path) {
        Ok(config) => match config.storage.ftp.get("default") {
            Some(ftp) => {
                println!("ftp profile: default");
                println!("ftp url: {}", ftp.url);
                println!("ftp remote dir: {}", ftp.remote_dir);
                println!(
                    "ftp credentials: {}",
                    if ftp.username.is_empty() || ftp.password.is_empty() {
                        "missing"
                    } else {
                        "configured"
                    }
                );
            }
            None if config.storage.ftp.is_empty() => println!("ftp profile: not configured"),
            None => println!("ftp profile configured but default profile is missing"),
        },
        Err(err) => println!("ftp config parse failed: {err:#}"),
    }
}

fn report_sftp_config(path: &std::path::Path) {
    if !path.exists() {
        return;
    }
    match storage::load_config(path) {
        Ok(config) => match config.storage.sftp.get("default") {
            Some(sftp) => {
                println!("sftp profile: default");
                println!("sftp host: {}:{}", sftp.host, sftp.port);
                println!("sftp remote dir: {}", sftp.remote_dir);
                println!("sftp auth: {:?}", sftp.auth);
                println!(
                    "sftp credentials: {}",
                    match sftp.auth {
                        storage::SftpAuth::Password if sftp.password.is_empty() => "missing",
                        storage::SftpAuth::PrivateKey if sftp.private_key_path.is_none() =>
                            "missing",
                        _ => "configured",
                    }
                );
            }
            None if config.storage.sftp.is_empty() => println!("sftp profile: not configured"),
            None => println!("sftp profile configured but default profile is missing"),
        },
        Err(err) => println!("sftp config parse failed: {err:#}"),
    }
}
