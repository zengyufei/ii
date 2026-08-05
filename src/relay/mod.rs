mod logging;
mod server;

#[cfg(test)]
use logging::LogFilter;
#[cfg(test)]
pub(crate) use server::build_server_config;
pub use server::run;
#[cfg(test)]
use server::{advertised_urls, load_tls_server_config};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        command::RelayArgs,
        transport::iroh::{EndpointPolicy, FILE_ALPN, bind_endpoint},
    };
    use std::{net::Ipv4Addr, time::Duration};
    use tokio::time::timeout;
    use tracing::{Level, level_filters::LevelFilter};
    fn unused_local_port() -> u16 {
        std::net::TcpListener::bind("127.0.0.1:0")
            .expect("bind temporary port")
            .local_addr()
            .expect("read temporary port")
            .port()
    }

    #[tokio::test]
    async fn http_relay_serves_probe_endpoints_and_stops() {
        rustls::crypto::ring::default_provider()
            .install_default()
            .ok();
        let requested_port = unused_local_port();
        let server = iroh_relay::server::Server::spawn(
            build_server_config(&RelayArgs {
                tls: false,
                domain: None,
                cert: None,
                key: None,
                port: Some(requested_port),
                bind: None,
            })
            .unwrap(),
        )
        .await
        .unwrap();
        let port = server.http_addr().unwrap().port();
        assert_eq!(port, requested_port);
        assert_eq!(
            reqwest::get(format!("http://127.0.0.1:{port}/generate_204"))
                .await
                .unwrap()
                .status(),
            reqwest::StatusCode::NO_CONTENT
        );
        server.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn http_relay_forwards_iroh_streams() {
        rustls::crypto::ring::default_provider()
            .install_default()
            .ok();
        let relay = iroh_relay::server::Server::spawn(
            build_server_config(&RelayArgs {
                tls: false,
                domain: None,
                cert: None,
                key: None,
                port: None,
                bind: None,
            })
            .unwrap(),
        )
        .await
        .unwrap();
        let relay_url: iroh::RelayUrl =
            format!("http://127.0.0.1:{}", relay.http_addr().unwrap().port())
                .parse()
                .unwrap();
        let receiver = bind_endpoint(
            EndpointPolicy::TrustedRelayOnly(relay_url.clone()),
            FILE_ALPN,
        )
        .await
        .unwrap();
        let sender = bind_endpoint(EndpointPolicy::TrustedRelayOnly(relay_url), FILE_ALPN)
            .await
            .unwrap();
        timeout(Duration::from_secs(5), receiver.online())
            .await
            .unwrap();
        timeout(Duration::from_secs(5), sender.online())
            .await
            .unwrap();

        let receiver_task = {
            let receiver = receiver.clone();
            tokio::spawn(async move {
                let incoming = timeout(Duration::from_secs(5), receiver.accept())
                    .await
                    .unwrap()
                    .unwrap();
                let connection = incoming.accept().unwrap().await.unwrap();
                let (_send, mut recv) = connection.accept_bi().await.unwrap();
                recv.read_to_end(64).await.unwrap()
            })
        };
        let connection = timeout(
            Duration::from_secs(5),
            sender.connect(receiver.addr(), FILE_ALPN),
        )
        .await
        .unwrap()
        .unwrap();
        let (mut send, _recv) = connection.open_bi().await.unwrap();
        send.write_all(b"relay over http").await.unwrap();
        send.finish().unwrap();
        assert_eq!(receiver_task.await.unwrap(), b"relay over http");

        connection.close(0u32.into(), b"done");
        sender.close().await;
        receiver.close().await;
        relay.shutdown().await.unwrap();
    }

    #[test]
    fn advertised_urls_never_use_unspecified_bind_address() {
        let (primary, other) = advertised_urls(
            "http",
            8443,
            None,
            Ipv4Addr::new(192, 168, 1, 20),
            vec![Ipv4Addr::new(10, 0, 0, 5)],
        );
        assert_eq!(primary, "http://192.168.1.20:8443");
        assert_eq!(other, ["http://10.0.0.5:8443"]);

        let (domain, other) = advertised_urls(
            "https",
            8443,
            Some("relay.example.com"),
            Ipv4Addr::new(192, 168, 1, 20),
            vec![Ipv4Addr::new(10, 0, 0, 5)],
        );
        assert_eq!(domain, "https://relay.example.com:8443");
        assert!(other.is_empty());
    }

    #[tokio::test]
    async fn generated_self_signed_tls_relay_serves_ping() {
        rustls::crypto::ring::default_provider()
            .install_default()
            .ok();
        let server = iroh_relay::server::Server::spawn(
            build_server_config(&RelayArgs {
                tls: true,
                domain: Some("relay.example.com".to_string()),
                cert: None,
                key: None,
                port: None,
                bind: None,
            })
            .unwrap(),
        )
        .await
        .unwrap();
        let port = server.https_addr().unwrap().port();
        let client = reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .build()
            .unwrap();
        assert_eq!(
            client
                .get(format!("https://127.0.0.1:{port}/ping"))
                .send()
                .await
                .unwrap()
                .status(),
            reqwest::StatusCode::OK
        );
        server.shutdown().await.unwrap();
    }

    #[test]
    fn malformed_manual_certificate_fails_clearly() {
        let temp = tempfile::tempdir().unwrap();
        let cert = temp.path().join("cert.pem");
        let key = temp.path().join("key.pem");
        std::fs::write(&cert, "not a certificate").unwrap();
        std::fs::write(&key, "not a key").unwrap();

        assert!(load_tls_server_config(&cert, &key).is_err());
    }

    #[test]
    fn log_filter_uses_default_and_longest_target_match() {
        let filter = LogFilter {
            default: LevelFilter::INFO,
            targets: vec![
                ("iroh".to_string(), LevelFilter::WARN),
                ("iroh_relay".to_string(), LevelFilter::DEBUG),
            ],
        };
        assert!(LogFilter::allows_level(LevelFilter::DEBUG, Level::DEBUG));
        assert!(!LogFilter::allows_level(LevelFilter::WARN, Level::INFO));
        assert!(LogFilter::allows_level(filter.default, Level::INFO));
        assert_eq!(
            filter
                .targets
                .iter()
                .filter(|(target, _)| "iroh_relay::client".starts_with(target))
                .max_by_key(|(target, _)| target.len())
                .map(|(_, level)| *level),
            Some(LevelFilter::DEBUG)
        );
    }
}
