use crate::cli::RelayArgs;
use anyhow::{Context, Result, bail};
#[cfg(feature = "relay-metrics")]
use iroh_relay::defaults::DEFAULT_METRICS_PORT;
use iroh_relay::{
    defaults::{DEFAULT_HTTP_PORT, DEFAULT_HTTPS_PORT, DEFAULT_RELAY_QUIC_PORT},
    server::{
        self, AcmeConfig, CertConfig, DEFAULT_CERT_RELOAD_INTERVAL, QuicConfig, reloading_resolver,
    },
};
use rustls_pki_types::pem::PemObject;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::signal;

const DEV_HTTP_PORT: u16 = 3340;
const DEFAULT_CONFIG_TEXT: &str = r#"# ii relay configuration
#
# Edit this file before running ii relay in production.

http_bind_addr = "0.0.0.0:80"
enable_quic_addr_discovery = true
enable_metrics = false

[tls]
https_bind_addr = "0.0.0.0:443"
quic_bind_addr = "0.0.0.0:7842"
cert_mode = "LetsEncrypt"
hostname = []
contact = ""
prod_tls = true
"#;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RelayFile {
    #[serde(default)]
    http_bind_addr: Option<SocketAddr>,
    #[serde(default)]
    tls: Option<TlsFile>,
    #[serde(default = "default_true")]
    enable_quic_addr_discovery: bool,
    #[serde(default)]
    enable_metrics: bool,
    #[serde(default)]
    metrics_bind_addr: Option<SocketAddr>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TlsFile {
    #[serde(default)]
    https_bind_addr: Option<SocketAddr>,
    #[serde(default)]
    quic_bind_addr: Option<SocketAddr>,
    #[serde(default)]
    hostname: Vec<String>,
    cert_mode: CertMode,
    #[serde(default)]
    cert_dir: Option<PathBuf>,
    #[serde(default)]
    manual_cert_path: Option<PathBuf>,
    #[serde(default)]
    manual_key_path: Option<PathBuf>,
    #[serde(default = "default_true")]
    prod_tls: bool,
    #[serde(default)]
    contact: Option<String>,
    #[serde(default)]
    dangerous_http_only: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
enum CertMode {
    Manual,
    LetsEncrypt,
    Reloading,
}

fn default_true() -> bool {
    true
}

pub fn default_config_path() -> Result<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let exe = std::env::current_exe().context("locate ii.exe")?;
        let dir = exe.parent().context("locate ii.exe directory")?;
        return Ok(dir.join("relay.toml"));
    }
    #[cfg(not(target_os = "windows"))]
    {
        Ok(PathBuf::from("/etc/ii/relay.toml"))
    }
}

pub async fn run(args: RelayArgs) -> Result<()> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .ok();

    let create_if_missing = args.config.is_none();
    let config_path = args.config.clone().unwrap_or(default_config_path()?);

    let mut cfg = load_or_create_config(&config_path, create_if_missing).await?;
    apply_cli_overrides(&mut cfg, &args);
    let server_cfg = build_server_config(cfg, args.dev).await?;
    let mut server = server::Server::spawn(server_cfg).await?;

    tokio::select! {
        _ = signal::ctrl_c() => {}
        _ = server.join() => {}
    }

    server.shutdown().await?;
    Ok(())
}

async fn load_or_create_config(path: &Path, create_if_missing: bool) -> Result<RelayFile> {
    if !path.exists() {
        if !create_if_missing {
            bail!("config file does not exist: {}", path.display());
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("create config directory {}", parent.display()))?;
        }
        fs::write(path, DEFAULT_CONFIG_TEXT)
            .with_context(|| format!("write default config {}", path.display()))?;
    }
    let text =
        fs::read_to_string(path).with_context(|| format!("read config {}", path.display()))?;
    let cfg: RelayFile = toml::from_str(&text).context("parse relay config")?;
    Ok(cfg)
}

fn apply_cli_overrides(cfg: &mut RelayFile, args: &RelayArgs) {
    if let Some(port) = args.http {
        cfg.http_bind_addr = Some(socket_addr(port));
    }
    if let Some(port) = args.metrics {
        cfg.enable_metrics = true;
        cfg.metrics_bind_addr = Some(socket_addr(port));
    }
    if let Some(port) = args.https {
        let tls = cfg.tls.get_or_insert_with(default_tls);
        tls.https_bind_addr = Some(socket_addr(port));
    }
    if let Some(port) = args.quic {
        let tls = cfg.tls.get_or_insert_with(default_tls);
        tls.quic_bind_addr = Some(socket_addr(port));
        cfg.enable_quic_addr_discovery = true;
    }
}

async fn build_server_config(cfg: RelayFile, dev: bool) -> Result<server::ServerConfig> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let http_bind_addr = if dev {
        cfg.http_bind_addr
            .unwrap_or_else(|| socket_addr(DEV_HTTP_PORT))
    } else {
        cfg.http_bind_addr
            .unwrap_or_else(|| socket_addr(DEFAULT_HTTP_PORT))
    };

    let mut relay_cfg = server::RelayConfig::new(http_bind_addr);

    let tls_file = if dev { None } else { cfg.tls.clone() };
    let tls_cfg = match tls_file.as_ref() {
        Some(tls) => Some(build_tls_config(tls, provider.clone()).await?),
        None => None,
    };

    let quic_cfg = if dev {
        None
    } else if cfg.enable_quic_addr_discovery {
        let tls = tls_file
            .as_ref()
            .context("QUIC address discovery requires TLS configuration")?;
        Some(QuicConfig::new(
            tls.quic_bind_addr
                .unwrap_or_else(|| socket_addr(DEFAULT_RELAY_QUIC_PORT)),
        ))
    } else {
        None
    };

    if dev {
        relay_cfg.tls = None;
    } else {
        relay_cfg.tls = tls_cfg;
    }

    let mut server_cfg = server::ServerConfig::default();
    server_cfg.relay = Some(relay_cfg);
    server_cfg.quic = quic_cfg;
    #[cfg(feature = "relay-metrics")]
    if cfg.enable_metrics {
        server_cfg.metrics_addr = Some(
            cfg.metrics_bind_addr
                .unwrap_or_else(|| socket_addr(DEFAULT_METRICS_PORT)),
        );
    }
    #[cfg(not(feature = "relay-metrics"))]
    if cfg.enable_metrics {
        bail!("relay metrics are not enabled in this build");
    }
    Ok(server_cfg)
}

async fn build_tls_config(
    tls: &TlsFile,
    provider: Arc<rustls::crypto::CryptoProvider>,
) -> Result<server::TlsConfig> {
    let cert = match tls.cert_mode {
        CertMode::Manual => {
            let cert_path = tls.manual_cert_path.clone().unwrap_or_else(|| {
                tls.cert_dir
                    .clone()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join("default.crt")
            });
            let key_path = tls.manual_key_path.clone().unwrap_or_else(|| {
                tls.cert_dir
                    .clone()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join("default.key")
            });
            let server_config = load_manual_server_config(&cert_path, &key_path, &provider)?;
            CertConfig::Manual { server_config }
        }
        CertMode::LetsEncrypt => {
            if tls.hostname.is_empty() {
                bail!("tls.hostname must not be empty for LetsEncrypt");
            }
            let contact = tls
                .contact
                .as_ref()
                .filter(|s| !s.is_empty())
                .context("tls.contact is required for LetsEncrypt")?;
            let acme = AcmeConfig::letsencrypt(tls.prod_tls)
                .domains(tls.hostname.clone())
                .contact(vec![format!("mailto:{contact}")])
                .cache_path(tls.cert_dir.clone().unwrap_or_else(|| PathBuf::from(".")));
            CertConfig::LetsEncrypt {
                acme_config: acme,
                server_config_builder: rustls_server_config_builder(&provider)?,
            }
        }
        CertMode::Reloading => {
            let cert_path = tls.manual_cert_path.clone().unwrap_or_else(|| {
                tls.cert_dir
                    .clone()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join("default.crt")
            });
            let key_path = tls.manual_key_path.clone().unwrap_or_else(|| {
                tls.cert_dir
                    .clone()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join("default.key")
            });
            let resolver = reloading_resolver(
                provider.as_ref(),
                cert_path,
                key_path,
                DEFAULT_CERT_RELOAD_INTERVAL,
            )
            .await?;
            let server_config =
                rustls_server_config_builder(&provider)?.with_cert_resolver(resolver);
            CertConfig::Manual { server_config }
        }
    };

    let https_bind_addr = tls
        .https_bind_addr
        .unwrap_or_else(|| socket_addr(DEFAULT_HTTPS_PORT));
    Ok(server::TlsConfig::new(https_bind_addr, cert))
}

fn rustls_server_config_builder(
    provider: &Arc<rustls::crypto::CryptoProvider>,
) -> Result<rustls::ConfigBuilder<rustls::ServerConfig, rustls::server::WantsServerCert>> {
    let builder = rustls::ServerConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .context("protocol versions")?
        .with_no_client_auth();
    Ok(builder)
}

fn load_manual_server_config(
    cert_path: &Path,
    key_path: &Path,
    provider: &Arc<rustls::crypto::CryptoProvider>,
) -> Result<rustls::ServerConfig> {
    let certs = rustls_pki_types::CertificateDer::pem_file_iter(cert_path)
        .with_context(|| format!("read certificate file {}", cert_path.display()))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("parse certificate chain")?;
    let key = rustls_pki_types::PrivateKeyDer::from_pem_file(key_path)
        .with_context(|| format!("read key file {}", key_path.display()))?;
    let config = rustls_server_config_builder(provider)?
        .with_single_cert(certs, key)
        .context("build rustls server config")?;
    Ok(config)
}

fn default_tls() -> TlsFile {
    TlsFile {
        https_bind_addr: None,
        quic_bind_addr: None,
        hostname: Vec::new(),
        cert_mode: CertMode::LetsEncrypt,
        cert_dir: None,
        manual_cert_path: None,
        manual_key_path: None,
        prod_tls: true,
        contact: None,
        dangerous_http_only: false,
    }
}

fn socket_addr(port: u16) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_path_is_platform_specific() {
        let path = default_config_path().unwrap();
        #[cfg(target_os = "windows")]
        assert!(path.ends_with("relay.toml"));
        #[cfg(not(target_os = "windows"))]
        assert_eq!(path, PathBuf::from("/etc/ii/relay.toml"));
    }
}
