use crate::cli::RelayArgs;
use anyhow::{Context, Result, bail};
#[cfg(feature = "relay-metrics")]
use iroh_relay::defaults::DEFAULT_METRICS_PORT;
use iroh_relay::server::{self, CertConfig};
use rustls::pki_types::pem::PemObject;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::signal;

const DEFAULT_PLAIN_HTTP_PORT: u16 = 3340;
const DEFAULT_TLS_PORT: u16 = 443;
const DEFAULT_CONFIG_TEXT: &str = r#"# ii relay configuration
#
# Default: plain HTTP relay reachable by IP address.
# For HTTPS, use: ii relay --tls relay.example.com --cert fullchain.pem --key privkey.pem

http_bind_addr = "0.0.0.0:3340"
enable_metrics = false
"#;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RelayFile {
    #[serde(default)]
    http_bind_addr: Option<SocketAddr>,
    #[serde(default)]
    tls: Option<TlsFile>,
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
    domain: Option<String>,
    #[serde(default, alias = "manual_cert_path")]
    cert_path: Option<PathBuf>,
    #[serde(default, alias = "manual_key_path")]
    key_path: Option<PathBuf>,
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
    if disable_incomplete_tls(&mut cfg) {
        eprintln!(
            "ii relay: ignored incomplete TLS settings; starting plain HTTP relay on port {} instead",
            cfg.http_bind_addr
                .unwrap_or_else(|| socket_addr(DEFAULT_PLAIN_HTTP_PORT))
                .port()
        );
    }
    apply_cli_overrides(&mut cfg, &args)?;
    let start_message = relay_start_message(&cfg);
    let server_cfg = build_server_config(cfg).await?;
    let mut server = server::Server::spawn(server_cfg).await?;
    eprintln!("{start_message}");

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

fn apply_cli_overrides(cfg: &mut RelayFile, args: &RelayArgs) -> Result<()> {
    if let Some(domain) = &args.tls_domain {
        cfg.tls = Some(TlsFile {
            https_bind_addr: args.port.map(socket_addr),
            domain: Some(domain.clone()),
            cert_path: args.cert.clone(),
            key_path: args.key.clone(),
        });
    } else if let Some(port) = args.port {
        cfg.http_bind_addr = Some(socket_addr(port));
    }
    if let Some(port) = args.metrics {
        cfg.enable_metrics = true;
        cfg.metrics_bind_addr = Some(socket_addr(port));
    }
    Ok(())
}

async fn build_server_config(cfg: RelayFile) -> Result<server::ServerConfig> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let http_bind_addr = if cfg.tls.is_some() {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)
    } else {
        cfg.http_bind_addr
            .unwrap_or_else(|| socket_addr(DEFAULT_PLAIN_HTTP_PORT))
    };

    let mut relay_cfg = server::RelayConfig::new(http_bind_addr);

    let tls_file = cfg.tls.clone();
    let tls_cfg = match tls_file.as_ref() {
        Some(tls) => Some(build_tls_config(tls, provider.clone()).await?),
        None => None,
    };

    relay_cfg.tls = tls_cfg;

    let mut server_cfg = server::ServerConfig::default();
    server_cfg.relay = Some(relay_cfg);
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
    let _domain = tls
        .domain
        .as_deref()
        .filter(|domain| !domain.is_empty())
        .context("TLS requires a domain")?;
    let cert_path = tls
        .cert_path
        .as_ref()
        .context("TLS requires a certificate path")?;
    let key_path = tls.key_path.as_ref().context("TLS requires a key path")?;
    let server_config = load_manual_server_config(cert_path, key_path, &provider)?;
    let cert = CertConfig::Manual { server_config };

    let https_bind_addr = tls
        .https_bind_addr
        .unwrap_or_else(|| socket_addr(DEFAULT_TLS_PORT));
    Ok(server::TlsConfig::new(https_bind_addr, cert))
}

fn relay_start_message(cfg: &RelayFile) -> String {
    match &cfg.tls {
        Some(tls) => format!(
            "ii relay: listening on https://{}:{}",
            tls.domain.as_deref().unwrap_or("<unknown>"),
            tls.https_bind_addr
                .unwrap_or_else(|| socket_addr(DEFAULT_TLS_PORT))
                .port()
        ),
        None => format!(
            "ii relay: listening on http://0.0.0.0:{}",
            cfg.http_bind_addr
                .unwrap_or_else(|| socket_addr(DEFAULT_PLAIN_HTTP_PORT))
                .port()
        ),
    }
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
    let certs = rustls::pki_types::CertificateDer::pem_file_iter(cert_path)
        .with_context(|| format!("read certificate file {}", cert_path.display()))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("parse certificate chain")?;
    let key = rustls::pki_types::PrivateKeyDer::from_pem_file(key_path)
        .with_context(|| format!("read key file {}", key_path.display()))?;
    let config = rustls_server_config_builder(provider)?
        .with_single_cert(certs, key)
        .context("build rustls server config")?;
    Ok(config)
}

fn disable_incomplete_tls(cfg: &mut RelayFile) -> bool {
    let should_disable = cfg.tls.as_ref().is_some_and(|tls| {
        tls.cert_path.is_none()
            || tls.key_path.is_none()
            || tls.domain.as_deref().is_none_or(str::is_empty)
    });
    if should_disable {
        cfg.tls = None;
        if cfg.http_bind_addr == Some(socket_addr(80)) {
            cfg.http_bind_addr = Some(socket_addr(DEFAULT_PLAIN_HTTP_PORT));
        }
    }
    should_disable
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

    #[test]
    fn default_config_is_plain_http_only() {
        let cfg: RelayFile = toml::from_str(DEFAULT_CONFIG_TEXT).unwrap();
        assert_eq!(
            cfg.http_bind_addr,
            Some(socket_addr(DEFAULT_PLAIN_HTTP_PORT))
        );
        assert!(cfg.tls.is_none());
    }

    #[test]
    fn incomplete_legacy_tls_config_falls_back_to_plain_http() {
        let mut cfg: RelayFile = toml::from_str(
            r#"
                http_bind_addr = "0.0.0.0:80"
                [tls]
                cert_mode = "LetsEncrypt"
                hostname = []
                contact = ""
            "#,
        )
        .unwrap();

        assert!(disable_incomplete_tls(&mut cfg));
        assert!(cfg.tls.is_none());
        assert_eq!(
            cfg.http_bind_addr,
            Some(socket_addr(DEFAULT_PLAIN_HTTP_PORT))
        );
    }

    #[test]
    fn tls_arguments_configure_https_and_hide_http_listener() {
        let mut cfg: RelayFile = toml::from_str(DEFAULT_CONFIG_TEXT).unwrap();
        let args = RelayArgs {
            port: Some(8443),
            tls_domain: Some("relay.example.com".to_string()),
            cert: Some(PathBuf::from("fullchain.pem")),
            key: Some(PathBuf::from("privkey.pem")),
            ..Default::default()
        };

        apply_cli_overrides(&mut cfg, &args).unwrap();
        let tls = cfg.tls.unwrap();
        assert_eq!(tls.https_bind_addr, Some(socket_addr(8443)));
        assert_eq!(tls.domain.as_deref(), Some("relay.example.com"));
    }
}
