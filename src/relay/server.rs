use super::{logging, state};
use crate::command::RelayArgs;
use anyhow::{Context, Result, bail};
use iroh_relay::server::{self, CertConfig};
use rustls::pki_types::pem::PemObject;
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::Path,
    sync::Arc,
};
use tokio::signal;
use tracing::info;

const DEFAULT_TLS_PORT: u16 = 443;

#[derive(Debug, Clone, Copy)]
enum RelayTlsMode {
    SelfSigned,
    Manual,
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

pub async fn run(args: RelayArgs) -> Result<()> {
    logging::install();
    rustls::crypto::ring::default_provider()
        .install_default()
        .ok();

    if let Some(public_url) = args.public {
        let bind_port = args.port.unwrap_or_else(|| {
            public_url
                .port_or_known_default()
                .expect("validated public URL")
        });
        let paths = state::paths(state::default_config_path()?);
        state::load_or_create(&paths, &public_url)?;
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
        state::relay_url_for_domain(&domain, bind_port, DEFAULT_TLS_PORT),
        bind_port,
        RelayTlsMode::Manual,
    )
    .await
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

pub(crate) fn build_server_config(
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

pub(crate) fn load_self_signed_server_config(
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
