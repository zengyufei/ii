use crate::cli::RelayArgs;
use anyhow::{Context, Result, bail};
use iroh_relay::server::{self, CertConfig};
use rcgen::generate_simple_self_signed;
use rustls::pki_types::pem::PemObject;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::signal;
use tracing::info;
use tracing_subscriber::EnvFilter;

const RELAY_STATE_VERSION: u8 = 1;
const CERT_FILE_NAME: &str = "relay-cert.pem";
const KEY_FILE_NAME: &str = "relay-key.pem";
const DEFAULT_TLS_PORT: u16 = 443;

#[derive(Debug, Clone, Copy)]
enum RelayTlsMode {
    SelfSigned,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RelayState {
    version: u8,
    public_url: String,
}

#[derive(Debug, Clone)]
struct RelayPaths {
    config_path: PathBuf,
    cert_path: PathBuf,
    key_path: PathBuf,
}

#[derive(Debug)]
struct RelayAccessLogger;

impl server::AccessControl for RelayAccessLogger {
    async fn on_connect(&self, request: &server::ClientRequest) -> server::Access {
        info!(
            endpoint = %request.endpoint_id(),
            connection = %request.connection_id(),
            "relay client connected"
        );
        server::Access::Allow
    }

    fn on_disconnect(&self, endpoint_id: iroh::EndpointId, connection_id: server::ConnectionId) {
        info!(
            endpoint = %endpoint_id,
            connection = %connection_id,
            "relay client disconnected"
        );
    }
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
    install_logging();
    rustls::crypto::ring::default_provider()
        .install_default()
        .ok();

    if let Some(public_url) = args.public {
        let bind_port = args.port.unwrap_or_else(|| {
            public_url
                .port_or_known_default()
                .expect("validated public URL")
        });
        let paths = relay_paths(default_config_path()?);
        load_or_create_state(&paths, &public_url)?;
        let server_config = load_self_signed_server_config(&paths.cert_path, &paths.key_path)
            .context("load persisted self-signed relay certificate")?;
        return run_server(
            server_config,
            public_url.to_string(),
            bind_port,
            RelayTlsMode::SelfSigned,
        )
        .await;
    }

    let domain = args
        .tls_domain
        .expect("CLI requires a relay mode before calling relay::run");
    let cert_path = args.cert.expect("CLI validates --cert for manual TLS mode");
    let key_path = args.key.expect("CLI validates --key for manual TLS mode");
    let bind_port = args.port.unwrap_or(DEFAULT_TLS_PORT);
    let server_config = load_self_signed_server_config(&cert_path, &key_path)
        .context("load manual TLS certificate")?;
    run_server(
        server_config,
        relay_url_for_domain(&domain, bind_port),
        bind_port,
        RelayTlsMode::Manual,
    )
    .await
}

fn relay_url_for_domain(domain: &str, port: u16) -> String {
    if port == DEFAULT_TLS_PORT {
        format!("https://{domain}")
    } else {
        format!("https://{domain}:{port}")
    }
}

async fn run_server(
    server_config: rustls::ServerConfig,
    public_url: String,
    bind_port: u16,
    tls_mode: RelayTlsMode,
) -> Result<()> {
    let mut server = server::Server::spawn(build_server_config(server_config, bind_port)?)
        .await
        .context("start HTTPS relay")?;

    eprintln!("ii relay: listening on {public_url}");
    eprintln!("ii relay: local HTTPS listener 0.0.0.0:{bind_port}");
    match tls_mode {
        RelayTlsMode::SelfSigned => {
            eprintln!("ii relay: self-signed TLS; clients must use ii send --relay <url> -k");
        }
        RelayTlsMode::Manual => {
            eprintln!("ii relay: manual TLS; clients use normal certificate verification");
        }
    }
    eprintln!("ii relay: relay-only mode; no UDP, QUIC, or direct peer path");

    tokio::select! {
        _ = signal::ctrl_c() => {
            eprintln!("ii relay: stopping");
        }
        result = server.join() => {
            result.context("relay server task failed")??;
        }
    }

    server.shutdown().await.context("stop relay")?;
    Ok(())
}

fn install_logging() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .compact()
        .try_init();
}

fn relay_paths(config_path: PathBuf) -> RelayPaths {
    let parent = config_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    RelayPaths {
        cert_path: parent.join(CERT_FILE_NAME),
        key_path: parent.join(KEY_FILE_NAME),
        config_path,
    }
}

fn load_or_create_state(paths: &RelayPaths, public_url: &iroh::RelayUrl) -> Result<RelayState> {
    if paths.config_path.exists() {
        let text = fs::read_to_string(&paths.config_path)
            .with_context(|| format!("read relay state {}", paths.config_path.display()))?;
        let state: RelayState = toml::from_str(&text).map_err(|err| {
            anyhow::anyhow!(
                "unsupported relay.toml for the self-signed relay mode; remove relay.toml, relay-cert.pem, and relay-key.pem together, then run ii relay --public <https-url>: {err}"
            )
        })?;
        if state.version != RELAY_STATE_VERSION {
            bail!("unsupported relay state version {}", state.version);
        }
        if state.public_url != public_url.as_str() {
            bail!(
                "relay state is bound to {}; requested {}. Remove relay.toml, relay-cert.pem, and relay-key.pem together to create a new relay identity",
                state.public_url,
                public_url
            );
        }
        match (paths.cert_path.exists(), paths.key_path.exists()) {
            (true, true) => return Ok(state),
            _ => bail!(
                "relay certificate state is incomplete: expected {} and {}",
                paths.cert_path.display(),
                paths.key_path.display()
            ),
        }
    }

    if paths.cert_path.exists() || paths.key_path.exists() {
        bail!(
            "relay certificate state is incomplete: relay.toml is missing but certificate material exists beside it"
        );
    }
    if let Some(parent) = paths.config_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create relay state directory {}", parent.display()))?;
    }

    let host = public_url
        .host_str()
        .context("public relay URL must include a host")?;
    let certified_key = generate_simple_self_signed(vec![host.to_string()])
        .context("generate self-signed relay certificate")?;
    fs::write(&paths.cert_path, certified_key.cert.pem())
        .with_context(|| format!("write relay certificate {}", paths.cert_path.display()))?;
    fs::write(&paths.key_path, certified_key.signing_key.serialize_pem())
        .with_context(|| format!("write relay key {}", paths.key_path.display()))?;
    set_private_key_permissions(&paths.key_path)?;

    let state = RelayState {
        version: RELAY_STATE_VERSION,
        public_url: public_url.to_string(),
    };
    let text = toml::to_string_pretty(&state).context("serialize relay state")?;
    fs::write(&paths.config_path, text)
        .with_context(|| format!("write relay state {}", paths.config_path.display()))?;
    Ok(state)
}

#[cfg(unix)]
fn set_private_key_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("set private key permissions {}", path.display()))
}

#[cfg(not(unix))]
fn set_private_key_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

fn build_server_config(
    server_config: rustls::ServerConfig,
    bind_port: u16,
) -> Result<server::ServerConfig> {
    let mut relay_config =
        server::RelayConfig::new(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0));
    relay_config.tls = Some(server::TlsConfig::new(
        SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), bind_port),
        CertConfig::Manual { server_config },
    ));
    relay_config.access = Arc::new(RelayAccessLogger);

    let mut config = server::ServerConfig::default();
    config.relay = Some(relay_config);
    Ok(config)
}

fn load_self_signed_server_config(
    cert_path: &Path,
    key_path: &Path,
) -> Result<rustls::ServerConfig> {
    let certs = rustls::pki_types::CertificateDer::pem_file_iter(cert_path)
        .with_context(|| format!("read certificate file {}", cert_path.display()))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("parse certificate chain")?;
    if certs.is_empty() {
        bail!("relay certificate file is empty: {}", cert_path.display());
    }
    let key = rustls::pki_types::PrivateKeyDer::from_pem_file(key_path)
        .with_context(|| format!("read key file {}", key_path.display()))?;
    rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .context("configure TLS protocol versions")?
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .context("build self-signed relay TLS config")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn public_url() -> iroh::RelayUrl {
        "https://127.0.0.1:8443".parse().unwrap()
    }

    #[test]
    fn default_path_is_platform_specific() {
        let path = default_config_path().unwrap();
        #[cfg(target_os = "windows")]
        assert!(path.ends_with("relay.toml"));
        #[cfg(not(target_os = "windows"))]
        assert_eq!(path, PathBuf::from("/etc/ii/relay.toml"));
    }

    #[test]
    fn first_setup_generates_and_reuses_certificate() {
        let temp = tempfile::tempdir().unwrap();
        let paths = relay_paths(temp.path().join("relay.toml"));

        let first = load_or_create_state(&paths, &public_url()).unwrap();
        let first_cert = fs::read(&paths.cert_path).unwrap();
        let first_key = fs::read(&paths.key_path).unwrap();
        let second = load_or_create_state(&paths, &public_url()).unwrap();

        assert_eq!(first.public_url, second.public_url);
        assert_eq!(first_cert, fs::read(&paths.cert_path).unwrap());
        assert_eq!(first_key, fs::read(&paths.key_path).unwrap());
        load_self_signed_server_config(&paths.cert_path, &paths.key_path).unwrap();
    }

    #[test]
    fn missing_certificate_material_fails_clearly() {
        let temp = tempfile::tempdir().unwrap();
        let paths = relay_paths(temp.path().join("relay.toml"));
        load_or_create_state(&paths, &public_url()).unwrap();
        fs::remove_file(&paths.key_path).unwrap();

        let err = load_or_create_state(&paths, &public_url()).unwrap_err();
        assert!(err.to_string().contains("incomplete"));
    }

    #[test]
    fn malformed_persisted_certificate_fails_clearly() {
        let temp = tempfile::tempdir().unwrap();
        let paths = relay_paths(temp.path().join("relay.toml"));
        load_or_create_state(&paths, &public_url()).unwrap();
        fs::write(&paths.cert_path, "not a certificate").unwrap();

        let err = load_self_signed_server_config(&paths.cert_path, &paths.key_path).unwrap_err();
        assert!(
            !err.to_string().is_empty(),
            "a malformed persisted certificate must return a clear error"
        );
    }

    #[test]
    fn state_rejects_a_changed_public_url() {
        let temp = tempfile::tempdir().unwrap();
        let paths = relay_paths(temp.path().join("relay.toml"));
        load_or_create_state(&paths, &public_url()).unwrap();
        let changed: iroh::RelayUrl = "https://relay.example.com".parse().unwrap();

        let err = load_or_create_state(&paths, &changed).unwrap_err();
        assert!(err.to_string().contains("bound to"));
    }

    #[test]
    fn legacy_relay_config_has_a_migration_error() {
        let temp = tempfile::tempdir().unwrap();
        let paths = relay_paths(temp.path().join("relay.toml"));
        fs::write(&paths.config_path, "http_bind_addr = \"0.0.0.0:3340\"").unwrap();

        let err = load_or_create_state(&paths, &public_url()).unwrap_err();
        assert!(err.to_string().contains("unsupported relay.toml"));
    }
}
