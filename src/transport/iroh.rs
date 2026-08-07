use crate::command::SendArgs;
use anyhow::{Context, Result, bail};
use iroh::{Endpoint, RelayMap, RelayMode, SecretKey, Watcher as _, endpoint::presets};
use iroh_relay::tls::CaTlsConfig;
use rustls::{
    DigitallySignedStruct, SignatureScheme,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    crypto::CryptoProvider,
    pki_types::{CertificateDer, ServerName, UnixTime},
};
use std::sync::Arc;
use std::{net::SocketAddr, time::Duration};

pub(crate) const FILE_ALPN: &[u8] = b"ii/file/1";
pub(crate) const TUNNEL_ALPN: &[u8] = b"ii/tunnel/1";

#[derive(Debug, Clone)]
pub(crate) enum EndpointPolicy {
    Standard(RelayMode),
    SelfSignedRelayOnly(iroh::RelayUrl),
    TrustedRelayOnly(iroh::RelayUrl),
    CustomRelayOnly {
        urls: Vec<iroh::RelayUrl>,
        self_signed: bool,
    },
}

impl EndpointPolicy {
    pub(crate) fn standard(relay_mode: RelayMode) -> Self {
        Self::Standard(relay_mode)
    }

    fn relay_mode(&self) -> RelayMode {
        match self {
            Self::Standard(mode) => mode.clone(),
            Self::SelfSignedRelayOnly(url) | Self::TrustedRelayOnly(url) => {
                RelayMode::Custom(RelayMap::from(url.clone()))
            }
            Self::CustomRelayOnly { urls, .. } => {
                RelayMode::Custom(RelayMap::from_iter(urls.clone()))
            }
        }
    }

    fn is_relay_only(&self) -> bool {
        matches!(
            self,
            Self::SelfSignedRelayOnly(_) | Self::TrustedRelayOnly(_) | Self::CustomRelayOnly { .. }
        )
    }

    fn accepts_self_signed_relay(&self) -> bool {
        matches!(self, Self::SelfSignedRelayOnly(_))
            || matches!(
                self,
                Self::CustomRelayOnly {
                    self_signed: true,
                    ..
                }
            )
    }
}

#[derive(Debug)]
struct AcceptAnyRelayCertificate {
    crypto_provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for AcceptAnyRelayCertificate {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer,
        _intermediates: &[CertificateDer],
        _server_name: &ServerName,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.crypto_provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

fn accept_self_signed_relay_tls() -> CaTlsConfig {
    CaTlsConfig::custom_server_cert_verifier(Arc::new(|crypto_provider| {
        Ok(Arc::new(AcceptAnyRelayCertificate { crypto_provider }))
    }))
}

pub(crate) async fn bind_endpoint(
    policy: EndpointPolicy,
    alpn: &[u8],
    quic_port: Option<u16>,
) -> Result<Endpoint> {
    let secret_key = SecretKey::generate();
    let mut builder = Endpoint::builder(presets::Minimal)
        .secret_key(secret_key)
        .alpns(vec![alpn.to_vec()])
        .relay_mode(policy.relay_mode());
    if policy.is_relay_only() {
        builder = builder.clear_ip_transports().clear_address_lookup();
    }
    if policy.accepts_self_signed_relay() {
        builder = builder.ca_tls_config(accept_self_signed_relay_tls());
    }
    if let Some(port) = quic_port {
        builder = builder
            .bind_addr(SocketAddr::from(([0, 0, 0, 0], port)))
            .context("bind fixed QUIC port")?;
    }
    builder.bind().await.context("bind endpoint")
}

pub(crate) fn endpoint_policy_for_send(
    args: &SendArgs,
    selected_relay: Option<&iroh::RelayUrl>,
) -> Result<EndpointPolicy> {
    if args.local || args.no_relay {
        return Ok(EndpointPolicy::standard(RelayMode::Disabled));
    }
    if let Some(url) = selected_relay.or_else(|| args.relay.first()) {
        return Ok(if args.accept_self_signed_relay {
            EndpointPolicy::SelfSignedRelayOnly(url.clone())
        } else {
            EndpointPolicy::TrustedRelayOnly(url.clone())
        });
    }
    Ok(EndpointPolicy::standard(RelayMode::Default))
}

#[derive(Debug, Clone)]
pub(crate) struct RelayProbe {
    pub(crate) url: iroh::RelayUrl,
    pub(crate) latency: Duration,
}

pub(crate) async fn probe_relays(
    urls: &[iroh::RelayUrl],
    self_signed: bool,
) -> Result<Vec<RelayProbe>> {
    if urls.len() < 2 {
        return Ok(Vec::new());
    }
    let endpoint = bind_endpoint(
        EndpointPolicy::CustomRelayOnly {
            urls: urls.to_vec(),
            self_signed,
        },
        FILE_ALPN,
        None,
    )
    .await
    .context("create relay probe endpoint")?;
    let report = tokio::time::timeout(Duration::from_secs(5), endpoint.net_report().initialized())
        .await
        .ok();
    endpoint.close().await;
    let Some(report) = report else {
        bail!("relay probe timed out")
    };
    let mut probes = urls
        .iter()
        .filter_map(|url| {
            report
                .relay_latency
                .iter()
                .filter(|(_, candidate, _)| *candidate == url)
                .map(|(_, candidate, latency)| RelayProbe {
                    url: candidate.clone(),
                    latency,
                })
                .min_by_key(|probe| probe.latency)
        })
        .collect::<Vec<_>>();
    probes.sort_by_key(|probe| probe.latency);
    if probes.is_empty() {
        bail!("all specified relays are unreachable")
    }
    Ok(probes)
}

pub(crate) fn should_wait_online(args: &SendArgs) -> bool {
    !args.local && !args.no_relay
}
