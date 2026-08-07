use anyhow::{Context, Result};

use crate::{
    command::DoctorArgs,
    storage,
    transport::iroh::{EndpointPolicy, FILE_ALPN, bind_endpoint},
};
use iroh::{RelayMode, Watcher as _};

pub async fn run(args: DoctorArgs) -> Result<()> {
    let s3_config_path = storage::default_config_path()?;
    println!("ii version: {}", env!("CARGO_PKG_VERSION"));
    println!("platform: {}", std::env::consts::OS);
    println!("s3 config: {}", s3_config_path.display());
    println!(
        "s3 config exists: {}",
        if s3_config_path.exists() { "yes" } else { "no" }
    );
    report_s3_config(&s3_config_path);
    report_r2_config(&s3_config_path);
    report_azure_config(&s3_config_path);
    report_webdav_config(&s3_config_path);
    report_ftp_config(&s3_config_path);
    report_sftp_config(&s3_config_path);

    println!("relay modes: HTTP by default; optional self-signed or manual TLS, always relay-only");
    println!("relay start: ii relay [--port <port>] [--tls]");
    if args.nat {
        report_nat().await?;
    }
    Ok(())
}

async fn report_nat() -> Result<()> {
    println!("nat probe: starting");
    let endpoint = bind_endpoint(
        EndpointPolicy::standard(RelayMode::Default),
        FILE_ALPN,
        None,
    )
    .await
    .context("create NAT probe endpoint")?;
    let bound = endpoint
        .bound_sockets()
        .into_iter()
        .map(|address| address.to_string())
        .collect::<Vec<_>>();
    println!(
        "udp sockets: {}",
        if bound.is_empty() {
            "none".to_string()
        } else {
            bound.join(", ")
        }
    );
    println!(
        "ipv4: {}",
        bound.iter().any(|address| address.contains('.'))
    );
    println!(
        "ipv6: {}",
        bound.iter().any(|address| address.contains(':'))
    );
    let report = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        endpoint.net_report().initialized(),
    )
    .await
    .ok();
    if let Some(report) = report {
        println!("udp report: v4={}, v6={}", report.udp_v4, report.udp_v6);
        println!(
            "nat mapping: ipv4={}, ipv6={}",
            report
                .mapping_varies_by_dest_ipv4
                .map_or("unknown".to_string(), |value| value.to_string()),
            report
                .mapping_varies_by_dest_ipv6
                .map_or("unknown".to_string(), |value| value.to_string())
        );
        println!("hairpin: unavailable (iroh does not expose a hairpin probe)");
        println!(
            "relay report: {}",
            report
                .preferred_relay
                .map(|relay| relay.to_string())
                .unwrap_or_else(|| "none".to_string())
        );
    } else {
        println!("udp report: unavailable");
        println!("nat mapping: unavailable");
        println!("hairpin: unavailable (iroh does not expose a hairpin probe)");
        println!("relay report: unavailable");
    }
    let relay_reachable =
        tokio::time::timeout(std::time::Duration::from_secs(5), endpoint.online())
            .await
            .is_ok();
    println!(
        "relay reachable: {}",
        if relay_reachable { "yes" } else { "no" }
    );
    endpoint.close().await;
    Ok(())
}

fn report_s3_config(path: &std::path::Path) {
    if !path.exists() {
        return;
    }
    match storage::load_config(path) {
        Ok(config) => {
            let profile = "default".to_string();
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

fn report_r2_config(path: &std::path::Path) {
    if !path.exists() {
        return;
    }
    match storage::load_config(path) {
        Ok(config) => match config.storage.r2.get("default") {
            Some(r2) => {
                println!("r2 profile: default");
                println!("r2 account: {}", r2.account_id);
                println!("r2 bucket: {}", r2.bucket);
                println!(
                    "r2 credentials: {}",
                    if r2.access_key_id.is_empty() || r2.secret_access_key.is_empty() {
                        "missing"
                    } else {
                        "configured"
                    }
                );
            }
            None if config.storage.r2.is_empty() => println!("r2 profile: not configured"),
            None => println!("r2 profile configured but default profile is missing"),
        },
        Err(err) => println!("r2 config parse failed: {err:#}"),
    }
}

fn report_azure_config(path: &std::path::Path) {
    if !path.exists() {
        return;
    }
    match storage::load_config(path) {
        Ok(config) => match config.storage.azure.get("default") {
            Some(azure) => {
                println!("azure profile: default");
                println!("azure account: {}", azure.account_name);
                println!("azure container: {}", azure.container);
                println!("azure auth: {:?}", azure.auth);
                println!(
                    "azure credentials: {}",
                    match azure.auth {
                        storage::AzureAuth::SharedKey if azure.account_key.is_empty() => "missing",
                        storage::AzureAuth::Sas if azure.sas_token.is_empty() => "missing",
                        _ => "configured",
                    }
                );
            }
            None if config.storage.azure.is_empty() => println!("azure profile: not configured"),
            None => println!("azure profile configured but default profile is missing"),
        },
        Err(err) => println!("azure config parse failed: {err:#}"),
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
