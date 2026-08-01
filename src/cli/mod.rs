use std::{path::PathBuf, process};

mod common;
mod help;
mod recv;
mod relay;
mod send;
mod tunnel;
mod web;
mod webrtc;

use common::*;
use help::*;

pub use crate::command::*;

impl Cli {
    pub fn parse() -> Self {
        match parse_args(std::env::args()) {
            Ok(cli) => cli,
            Err(ParseAction::Print { text, code }) => {
                if code == 0 {
                    println!("{text}");
                } else {
                    eprintln!("{text}");
                }
                process::exit(code);
            }
        }
    }

    #[cfg(test)]
    pub fn parse_from<I, T>(args: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<String>,
    {
        match parse_args(args.into_iter().map(Into::into)) {
            Ok(cli) => cli,
            Err(ParseAction::Print { text, code }) => panic!("parse exited with {code}: {text}"),
        }
    }
}

fn parse_args<I, T>(args: I) -> Result<Cli, ParseAction>
where
    I: IntoIterator<Item = T>,
    T: Into<String>,
{
    let mut args: Vec<String> = args.into_iter().map(Into::into).collect();
    if !args.is_empty() {
        args.remove(0);
    }

    let Some(command) = args.first().cloned() else {
        return Err(ParseAction::help(HELP));
    };

    if is_help(&command) {
        return Err(ParseAction::help(HELP));
    }
    if command == "--version" || command == "-V" {
        return Err(ParseAction::version());
    }

    let rest = args.split_off(1);
    let command = match command.as_str() {
        "send" => Command::Send(send::parse(rest)?),
        "web" => Command::Web(web::parse(rest)?),
        "webrtc" => Command::Webrtc(webrtc::parse(rest)?),
        "tunnel" => Command::Tunnel(tunnel::parse(rest)?),
        "recv" => Command::Recv(recv::parse(rest)?),
        "relay" => Command::Relay(relay::parse(rest)?),
        "doctor" => reject_extra("doctor", rest).map(|_| Command::Doctor)?,
        "version" => reject_extra("version", rest).map(|_| Command::Version)?,
        other => return Err(ParseAction::error(format!("unknown command `{other}`"))),
    };

    Ok(Cli { command })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recv_accepts_trace() {
        let cli = Cli::parse_from(["ii", "recv", "ii1abc", "--trace"]);
        match cli.command {
            Command::Recv(args) => assert!(args.trace),
            _ => panic!("expected recv command"),
        }
    }

    #[test]
    fn send_accepts_keep_alive() {
        let cli = Cli::parse_from(["ii", "send", "file.txt", "-t"]);
        match cli.command {
            Command::Send(args) => assert!(args.keep_alive),
            _ => panic!("expected send command"),
        }
    }

    #[test]
    fn send_accepts_copy_and_output() {
        let cli = Cli::parse_from(["ii", "send", "file.txt", "-c", "-o", "recv.txt"]);
        match cli.command {
            Command::Send(args) => {
                assert!(args.copy);
                assert_eq!(args.output, Some(PathBuf::from("recv.txt")));
            }
            _ => panic!("expected send command"),
        }
    }

    #[test]
    fn send_accepts_s3_delete_after_recv() {
        let cli = Cli::parse_from(["ii", "send", "--s3", "-d", "file.txt"]);
        match cli.command {
            Command::Send(args) => {
                assert!(args.s3);
                assert!(args.delete_after_recv);
            }
            _ => panic!("expected send command"),
        }
    }

    #[test]
    fn send_accepts_webdav_delete_after_recv() {
        let cli = Cli::parse_from(["ii", "send", "--webdav", "-d", "file.txt"]);
        match cli.command {
            Command::Send(args) => {
                assert!(args.webdav);
                assert!(args.delete_after_recv);
            }
            _ => panic!("expected send command"),
        }
    }

    #[test]
    fn send_accepts_webdav_portable() {
        let cli = Cli::parse_from(["ii", "send", "--webdav", "-p", "file.txt"]);
        match cli.command {
            Command::Send(args) => {
                assert!(args.webdav);
                assert!(args.portable_webdav);
            }
            _ => panic!("expected send command"),
        }
    }

    #[test]
    fn send_accepts_web_for_a_file_or_folder() {
        let cli = Cli::parse_from(["ii", "send", "photos", "--web"]);
        match cli.command {
            Command::Send(args) => {
                assert!(args.web);
                assert_eq!(args.path, Some(PathBuf::from("photos")));
            }
            _ => panic!("expected send command"),
        }
    }

    #[test]
    fn send_rejects_web_without_a_path_or_with_another_mode() {
        assert!(matches!(
            parse_args(["ii", "send", "--web"]),
            Err(ParseAction::Print { code: 2, .. })
        ));
        assert!(matches!(
            parse_args(["ii", "send", "file.txt", "--web", "--local"]),
            Err(ParseAction::Print { code: 2, .. })
        ));
    }

    #[test]
    fn send_accepts_web_token() {
        let token = "A1b2C3d4E5f6G7h8";
        for args in [
            vec!["ii", "send", "file.txt", "--web", "--token", token],
            vec![
                "ii",
                "send",
                "file.txt",
                "--web",
                "--token=A1b2C3d4E5f6G7h8",
            ],
        ] {
            let cli = Cli::parse_from(args);
            match cli.command {
                Command::Send(args) => assert_eq!(args.web_token.as_deref(), Some(token)),
                _ => panic!("expected send command"),
            }
        }
    }

    #[test]
    fn send_accepts_web_upload_path() {
        for args in [
            vec!["ii", "send", "file.txt", "--web", "--path", "uploads"],
            vec!["ii", "send", "file.txt", "--web", "--path=uploads"],
        ] {
            let cli = Cli::parse_from(args);
            match cli.command {
                Command::Send(args) => {
                    assert_eq!(args.web_upload_dir, Some(PathBuf::from("uploads")));
                }
                _ => panic!("expected send command"),
            }
        }
    }

    #[test]
    fn web_accepts_directory_token_and_upload_path() {
        let token = "A1b2C3d4E5f6G7h8";
        for args in [
            vec!["ii", "web"],
            vec!["ii", "web", "shared", "--token", token, "--path", "uploads"],
            vec![
                "ii",
                "web",
                "shared",
                "--token=A1b2C3d4E5f6G7h8",
                "--path=uploads",
            ],
        ] {
            let cli = Cli::parse_from(args);
            match cli.command {
                Command::Web(args) => {
                    if args.dir.is_some() {
                        assert_eq!(args.dir, Some(PathBuf::from("shared")));
                        assert_eq!(args.web_token.as_deref(), Some(token));
                        assert_eq!(args.web_upload_dir, Some(PathBuf::from("uploads")));
                    }
                }
                _ => panic!("expected web command"),
            }
        }
    }

    #[test]
    fn web_rejects_invalid_options_and_multiple_directories() {
        for args in [
            vec!["ii", "web", "first", "second"],
            vec!["ii", "web", "-p"],
            vec!["ii", "web", "--token", "too-short"],
            vec!["ii", "web", "--token", "A1b2C3d4E5f6G7h!"],
            vec!["ii", "web", "--token"],
            vec!["ii", "web", "--path"],
        ] {
            assert!(matches!(
                parse_args(args),
                Err(ParseAction::Print { code: 2, .. })
            ));
        }
    }

    #[test]
    fn web_token_accepts_hyphen_and_underscore() {
        assert!(is_valid_web_token("A1b2C3d4E5f6G_-h"));
    }

    #[test]
    fn send_rejects_invalid_web_tokens() {
        for args in [
            vec!["ii", "send", "file.txt", "--token", "A1b2C3d4E5f6G7h8"],
            vec!["ii", "send", "file.txt", "--web", "--token"],
            vec!["ii", "send", "file.txt", "--web", "--token", "too-short"],
            vec![
                "ii",
                "send",
                "file.txt",
                "--web",
                "--token",
                "A1b2C3d4E5f6G7h!",
            ],
            vec![
                "ii",
                "send",
                "file.txt",
                "--web",
                "--token",
                &"a".repeat(129),
            ],
        ] {
            assert!(matches!(
                parse_args(args),
                Err(ParseAction::Print { code: 2, .. })
            ));
        }
    }

    #[test]
    fn send_rejects_web_upload_path_without_web() {
        assert!(matches!(
            parse_args(["ii", "send", "file.txt", "--path", "uploads"]),
            Err(ParseAction::Print { code: 2, .. })
        ));
    }

    #[test]
    fn webrtc_accepts_optional_token() {
        let token = "A1b2C3d4E5f6G7h8";
        for args in [
            vec!["ii", "webrtc"],
            vec!["ii", "webrtc", "--token", token],
            vec!["ii", "webrtc", "--token=A1b2C3d4E5f6G7h8"],
        ] {
            let cli = Cli::parse_from(args);
            match cli.command {
                Command::Webrtc(args) => {
                    if args.web_token.is_some() {
                        assert_eq!(args.web_token.as_deref(), Some(token));
                    }
                }
                _ => panic!("expected webrtc command"),
            }
        }
    }

    #[test]
    fn webrtc_rejects_invalid_or_unrelated_arguments() {
        for args in [
            vec!["ii", "webrtc", "shared"],
            vec!["ii", "webrtc", "-p"],
            vec!["ii", "webrtc", "--path", "uploads"],
            vec!["ii", "webrtc", "--token", "too-short"],
            vec!["ii", "webrtc", "--token"],
        ] {
            assert!(matches!(
                parse_args(args),
                Err(ParseAction::Print { code: 2, .. })
            ));
        }
    }

    #[test]
    fn tunnel_parses_serve_and_connect_modes() {
        let serve = Cli::parse_from([
            "ii",
            "tunnel",
            "-s",
            "nas.example.com:22",
            "--relay",
            "https://relay.example.com:8443",
            "-k",
        ]);
        match serve.command {
            Command::Tunnel(TunnelArgs::Serve {
                target,
                relay,
                accept_self_signed_relay,
            }) => {
                assert_eq!(target, "nas.example.com:22");
                assert_eq!(
                    relay.unwrap().to_string(),
                    "https://relay.example.com:8443/"
                );
                assert!(accept_self_signed_relay);
            }
            _ => panic!("expected tunnel serve command"),
        }

        let connect = Cli::parse_from(["ii", "tunnel", "-c", "ii1ticket", "--listen=0.0.0.0:8022"]);
        match connect.command {
            Command::Tunnel(TunnelArgs::Connect { ticket, listen }) => {
                assert_eq!(ticket, "ii1ticket");
                assert_eq!(listen.unwrap(), "0.0.0.0:8022".parse().unwrap());
            }
            _ => panic!("expected tunnel connect command"),
        }
    }

    #[test]
    fn tunnel_rejects_invalid_mode_combinations_and_addresses() {
        for args in [
            vec!["ii", "tunnel"],
            vec!["ii", "tunnel", "-s", "host:22", "-c", "ii1ticket"],
            vec![
                "ii",
                "tunnel",
                "-c",
                "ii1ticket",
                "--relay",
                "https://relay.example.com",
            ],
            vec![
                "ii",
                "tunnel",
                "-s",
                "host:22",
                "--listen",
                "127.0.0.1:8080",
            ],
            vec!["ii", "tunnel", "-s", "host:22", "-k"],
            vec!["ii", "tunnel", "-s", "::1:22"],
            vec!["ii", "tunnel", "-s", "host:0"],
            vec![
                "ii",
                "tunnel",
                "-c",
                "ii1ticket",
                "--listen",
                "localhost:8080",
            ],
        ] {
            assert!(
                matches!(
                    parse_args(args.clone()),
                    Err(ParseAction::Print { code: 2, .. })
                ),
                "expected rejection for {args:?}"
            );
        }
    }

    #[test]
    fn send_accepts_ftp_and_sftp_storage_options() {
        let ftp = Cli::parse_from(["ii", "send", "--ftp", "-p", "-d", "file.txt"]);
        match ftp.command {
            Command::Send(args) => {
                assert!(args.ftp);
                assert!(args.portable_webdav);
                assert!(args.delete_after_recv);
            }
            _ => panic!("expected send command"),
        }

        let sftp = Cli::parse_from(["ii", "send", "--sftp", "--profile", "server", "file.txt"]);
        match sftp.command {
            Command::Send(args) => {
                assert!(args.sftp);
                assert_eq!(args.profile.as_deref(), Some("server"));
            }
            _ => panic!("expected send command"),
        }
    }

    #[test]
    fn send_rejects_conflicting_storage_backends_and_orphaned_portable_flag() {
        let conflicting = parse_args(["ii", "send", "--ftp", "--sftp", "file.txt"]);
        let portable_without_backend = parse_args(["ii", "send", "-p", "file.txt"]);
        assert!(matches!(
            conflicting,
            Err(ParseAction::Print { code: 2, .. })
        ));
        assert!(matches!(
            portable_without_backend,
            Err(ParseAction::Print { code: 2, .. })
        ));
    }

    #[test]
    fn root_help_contract_keeps_exit_code_and_usage() {
        match parse_args(["ii"]) {
            Err(ParseAction::Print { text, code }) => {
                assert_eq!(code, 0);
                assert_eq!(text, HELP);
            }
            Ok(_) => panic!("expected help"),
        }
    }

    #[test]
    fn conflicting_backends_keep_exit_code_and_error_text() {
        match parse_args(["ii", "send", "--ftp", "--sftp", "file.txt"]) {
            Err(ParseAction::Print { text, code }) => {
                assert_eq!(code, 2);
                assert!(text.starts_with("error: --s3, --webdav, --ftp, --sftp, --web, --local, --relay and --no-relay conflict with each other\n\n"));
                assert!(text.ends_with(HELP));
            }
            Ok(_) => panic!("expected conflicting backends to fail"),
        }
    }

    #[test]
    fn command_help_contracts_keep_existing_text() {
        for (command, expected) in [
            ("send", SEND_HELP),
            ("web", WEB_HELP),
            ("webrtc", WEBRTC_HELP),
            ("tunnel", TUNNEL_HELP),
            ("recv", RECV_HELP),
            ("relay", RELAY_HELP),
        ] {
            match parse_args(["ii", command, "--help"]) {
                Err(ParseAction::Print { text, code }) => {
                    assert_eq!(code, 0, "{command}");
                    assert_eq!(text, expected, "{command}");
                }
                Ok(_) => panic!("expected {command} help"),
            }
        }
    }

    #[test]
    fn send_accepts_backend_profile() {
        let cli = Cli::parse_from(["ii", "send", "--s3", "--profile", "work", "file.txt"]);
        match cli.command {
            Command::Send(args) => {
                assert!(args.s3);
                assert_eq!(args.profile, Some("work".to_string()));
            }
            _ => panic!("expected send command"),
        }
    }

    #[test]
    fn send_accepts_self_signed_relay_flag_only_with_relay() {
        let cli = Cli::parse_from([
            "ii",
            "send",
            "file.txt",
            "--relay",
            "https://127.0.0.1:8443",
            "-k",
        ]);
        match cli.command {
            Command::Send(args) => assert!(args.accept_self_signed_relay),
            _ => panic!("expected send command"),
        }

        let result = parse_args(["ii", "send", "file.txt", "-k"]);
        assert!(matches!(result, Err(ParseAction::Print { code: 2, .. })));
    }

    #[test]
    fn relay_accepts_public_https_url() {
        let cli = Cli::parse_from([
            "ii",
            "relay",
            "--public",
            "https://relay.example.com:8443",
            "-H",
            "8443",
        ]);
        match cli.command {
            Command::Relay(args) => {
                assert_eq!(args.port, Some(8443));
                assert_eq!(
                    args.public.as_ref().map(|url| url.as_str()),
                    Some("https://relay.example.com:8443/")
                );
                assert!(args.tls_domain.is_none());
            }
            _ => panic!("expected relay command"),
        }
    }

    #[test]
    fn relay_accepts_manual_tls_mode() {
        let cli = Cli::parse_from([
            "ii",
            "relay",
            "--tls",
            "relay.example.com",
            "--cert",
            "fullchain.pem",
            "--key",
            "privkey.pem",
            "-H",
            "8443",
        ]);
        match cli.command {
            Command::Relay(args) => {
                assert!(args.public.is_none());
                assert_eq!(args.tls_domain.as_deref(), Some("relay.example.com"));
                assert_eq!(args.cert, Some(PathBuf::from("fullchain.pem")));
                assert_eq!(args.key, Some(PathBuf::from("privkey.pem")));
                assert_eq!(args.port, Some(8443));
            }
            _ => panic!("expected relay command"),
        }
    }

    #[test]
    fn relay_rejects_missing_mode() {
        let result = parse_args(["ii", "relay"]);
        assert!(matches!(result, Err(ParseAction::Print { code: 2, .. })));
    }

    #[test]
    fn relay_rejects_non_https_public_url() {
        let result = parse_args(["ii", "relay", "--public", "http://127.0.0.1:3340"]);
        assert!(matches!(result, Err(ParseAction::Print { code: 2, .. })));
    }

    #[test]
    fn relay_rejects_zero_ports_and_conflicting_modes() {
        let invalid_public_port = parse_args(["ii", "relay", "--public", "https://127.0.0.1:0"]);
        let invalid_bind_port = parse_args([
            "ii",
            "relay",
            "--public",
            "https://127.0.0.1:8443",
            "-H",
            "0",
        ]);
        let conflicting_modes = parse_args([
            "ii",
            "relay",
            "--public",
            "https://127.0.0.1:8443",
            "--tls",
            "relay.example.com",
        ]);

        assert!(matches!(
            invalid_public_port,
            Err(ParseAction::Print { code: 2, .. })
        ));
        assert!(matches!(
            invalid_bind_port,
            Err(ParseAction::Print { code: 2, .. })
        ));
        assert!(matches!(
            conflicting_modes,
            Err(ParseAction::Print { code: 2, .. })
        ));
    }

    #[test]
    fn send_rejects_non_https_custom_relay() {
        let result = parse_args(["ii", "send", "file.txt", "--relay", "http://127.0.0.1"]);
        assert!(matches!(result, Err(ParseAction::Print { code: 2, .. })));
    }
}
