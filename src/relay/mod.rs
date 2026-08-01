mod logging;
mod server;
mod state;

#[cfg(test)]
use logging::LogFilter;
pub use server::run;
#[cfg(test)]
use server::{build_server_config, load_self_signed_server_config};
pub use state::default_config_path;
#[cfg(test)]
use state::{load_or_create as load_or_create_state, paths as relay_paths};

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tracing::{Level, level_filters::LevelFilter};
    fn unused_local_port() -> u16 {
        std::net::TcpListener::bind("127.0.0.1:0")
            .expect("bind temporary port")
            .local_addr()
            .expect("read temporary port")
            .port()
    }

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

    #[tokio::test]
    async fn minimal_relay_serves_probe_endpoints_and_stops() {
        rustls::crypto::ring::default_provider()
            .install_default()
            .ok();
        let temp = tempfile::tempdir().unwrap();
        let paths = relay_paths(temp.path().join("relay.toml"));
        load_or_create_state(&paths, &public_url()).unwrap();
        let server_config =
            load_self_signed_server_config(&paths.cert_path, &paths.key_path).unwrap();
        let server = iroh_relay::server::Server::spawn(
            build_server_config(server_config, unused_local_port()).unwrap(),
        )
        .await
        .unwrap();

        let https = server.https_addr().unwrap();
        let client = reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .build()
            .unwrap();
        assert_eq!(
            client
                .get(format!("https://127.0.0.1:{}/ping", https.port()))
                .send()
                .await
                .unwrap()
                .status(),
            reqwest::StatusCode::OK
        );

        let http = server.http_addr().unwrap();
        assert_eq!(
            reqwest::get(format!("http://{http}/generate_204"))
                .await
                .unwrap()
                .status(),
            reqwest::StatusCode::NO_CONTENT
        );
        server.shutdown().await.unwrap();
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
