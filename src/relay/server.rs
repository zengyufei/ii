use super::logging;
use crate::{command::RelayArgs, web::lan_ipv4_hosts};
use anyhow::{Context, Result, bail};
use iroh_relay::server::{self, CertConfig};
use rcgen::generate_simple_self_signed;
use rustls::pki_types::pem::PemObject;
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::Path,
    sync::Arc,
};
use tokio::signal;
use tracing::info;

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

    let mut server = server::Server::spawn(build_server_config(&args)?)
        .await
        .context("start relay")?;
    let port = if args.tls {
        server
            .https_addr()
            .context("read HTTPS relay listener address")?
            .port()
    } else {
        server
            .http_addr()
            .context("read HTTP relay listener address")?
            .port()
    };

    print_addresses(&args, port);

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

fn print_addresses(args: &RelayArgs, port: u16) {
    let scheme = if args.tls { "https" } else { "http" };
    let (primary, other) = lan_ipv4_hosts();
    let (url, other_urls) = advertised_urls(scheme, port, args.domain.as_deref(), primary, other);
    eprintln!("ii relay: {url}");
    if args.domain.is_none() {
        eprintln!();
        eprintln!("other:");
        for url in other_urls {
            eprintln!("{url}");
        }
    }
    eprintln!();
    if args.tls && args.cert.is_none() {
        eprintln!("ii relay: self-signed TLS; clients must use ii send --relay <url> -k");
    }
    eprintln!("ii relay: relay-only mode; no UDP, QUIC, or direct peer path");
    eprintln!();
    eprintln!("press Ctrl+C to stop relay");
}

pub(super) fn advertised_urls(
    scheme: &str,
    port: u16,
    domain: Option<&str>,
    primary: Ipv4Addr,
    other: Vec<Ipv4Addr>,
) -> (String, Vec<String>) {
    if let Some(domain) = domain {
        return (format!("{scheme}://{domain}:{port}"), Vec::new());
    }
    (
        format!("{scheme}://{primary}:{port}"),
        other
            .into_iter()
            .map(|host| format!("{scheme}://{host}:{port}"))
            .collect(),
    )
}

pub(crate) fn build_server_config(args: &RelayArgs) -> Result<server::ServerConfig> {
    let bind_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), args.port.unwrap_or(0));
    let mut relay_config = if args.tls {
        // TLS mode needs a separate local HTTP listener for captive-portal responses.
        server::RelayConfig::new(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0))
    } else {
        server::RelayConfig::new(bind_addr)
    };
    if args.tls {
        relay_config.tls = Some(server::TlsConfig::new(
            bind_addr,
            CertConfig::Manual {
                server_config: tls_server_config(args)?,
            },
        ));
    }
    relay_config.access = Arc::new(RelayAccessLogger);

    let mut config = server::ServerConfig::default();
    config.relay = Some(relay_config);
    Ok(config)
}

fn tls_server_config(args: &RelayArgs) -> Result<rustls::ServerConfig> {
    match (&args.cert, &args.key) {
        (Some(cert), Some(key)) => load_tls_server_config(cert, key),
        (None, None) => generate_self_signed_server_config(args.domain.as_deref()),
        _ => bail!("--cert and --key must be provided together"),
    }
}

fn generate_self_signed_server_config(domain: Option<&str>) -> Result<rustls::ServerConfig> {
    let certified_key =
        generate_simple_self_signed(vec![domain.unwrap_or("localhost").to_string()])
            .context("generate self-signed relay certificate")?;
    let cert = rustls::pki_types::CertificateDer::from(certified_key.cert.der().clone());
    let key =
        rustls::pki_types::PrivateKeyDer::Pkcs8(certified_key.signing_key.serialize_der().into());
    build_tls_server_config(vec![cert], key)
}

pub(crate) fn load_tls_server_config(
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
    build_tls_server_config(certs, key)
}

fn build_tls_server_config(
    certs: Vec<rustls::pki_types::CertificateDer<'static>>,
    key: rustls::pki_types::PrivateKeyDer<'static>,
) -> Result<rustls::ServerConfig> {
    rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .with_safe_default_protocol_versions()
        .context("configure TLS protocol versions")?
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .context("build relay TLS config")
}
