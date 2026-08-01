use crate::command::SendArgs;
use anyhow::{Context, Result};
use iroh::{Endpoint, RelayMap, RelayMode, SecretKey, endpoint::presets};
use iroh_relay::tls::CaTlsConfig;
use rustls::{
    DigitallySignedStruct, SignatureScheme,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    crypto::CryptoProvider,
    pki_types::{CertificateDer, ServerName, UnixTime},
};
use std::sync::Arc;

pub(crate) const FILE_ALPN: &[u8] = b"ii/file/1";
pub(crate) const TUNNEL_ALPN: &[u8] = b"ii/tunnel/1";

#[derive(Debug, Clone)]
pub(crate) enum EndpointPolicy {
    Standard(RelayMode),
    SelfSignedRelayOnly(iroh::RelayUrl),
    TrustedRelayOnly(iroh::RelayUrl),
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
        }
    }

    fn is_relay_only(&self) -> bool {
        matches!(
            self,
            Self::SelfSignedRelayOnly(_) | Self::TrustedRelayOnly(_)
        )
    }

    fn accepts_self_signed_relay(&self) -> bool {
        matches!(self, Self::SelfSignedRelayOnly(_))
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

pub(crate) async fn bind_endpoint(policy: EndpointPolicy, alpn: &[u8]) -> Result<Endpoint> {
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
    builder.bind().await.context("bind endpoint")
}

pub(crate) fn endpoint_policy_for_send(args: &SendArgs) -> Result<EndpointPolicy> {
    if args.local || args.no_relay {
        return Ok(EndpointPolicy::standard(RelayMode::Disabled));
    }
    if let Some(url) = &args.relay {
        return Ok(if args.accept_self_signed_relay {
            EndpointPolicy::SelfSignedRelayOnly(url.clone())
        } else {
            EndpointPolicy::TrustedRelayOnly(url.clone())
        });
    }
    Ok(EndpointPolicy::standard(RelayMode::Default))
}

pub(crate) fn should_wait_online(args: &SendArgs) -> bool {
    !args.local && !args.no_relay
}
