use std::{path::PathBuf, process};

mod common;
mod dav;
mod discover;
mod help;
mod jobs;
mod network;
mod recv;
mod relay;
mod send;
mod socks5;
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
        "help" => return Err(help_for(rest)),
        "send" => Command::Send(send::parse(rest)?),
        "watch" => Command::Watch(jobs::parse_watch(rest)?),
        "queue" => Command::Queue(jobs::parse_queue(rest)?),
        "web" => Command::Web(web::parse(rest)?),
        "dav" => Command::Dav(dav::parse(rest)?),
        "socks5" => Command::Socks5(socks5::parse(rest)?),
        "http" => Command::Http(network::http(rest)?),
        "paste" => Command::Paste(network::paste(rest)?),
        "drop" => Command::Drop(network::drop(rest)?),
        "proxy" => Command::Proxy(network::proxy(rest)?),
        "tcp" => Command::Tcp(network::tcp(rest)?),
        "udp" => Command::Udp(network::udp(rest)?),
        "ping" => Command::Ping(network::ping(rest)?),
        "speed" => Command::Speed(network::speed(rest)?),
        "wake" => Command::Wake(network::wake(rest)?),
        "port" => Command::Port(network::port(rest)?),
        "health" => Command::Health(network::health(rest)?),
        "pac" => Command::Pac(network::pac(rest)?),
        "webrtc" => Command::Webrtc(webrtc::parse(rest)?),
        "tunnel" => Command::Tunnel(tunnel::parse(rest)?),
        "recv" => Command::Recv(recv::parse(rest)?),
        "relay" => Command::Relay(relay::parse(rest)?),
        "discover" => Command::Discover(discover::parse(rest)?),
        "doctor" => Command::Doctor(parse_doctor(rest)?),
        "version" => reject_extra("version", rest).map(|_| Command::Version)?,
        other => return Err(ParseAction::error(format!("unknown command `{other}`"))),
    };

    Ok(Cli { command })
}

fn parse_doctor(args: Vec<String>) -> Result<DoctorArgs, ParseAction> {
    let mut out = DoctorArgs::default();
    for arg in args {
        match arg.as_str() {
            "--nat" => {
                if out.nat {
                    return Err(ParseAction::error("--nat may be specified only once"));
                }
                out.nat = true;
            }
            value if is_help(value) => return Err(ParseAction::help(DOCTOR_HELP)),
            value => {
                return Err(ParseAction::error(format!(
                    "`doctor` does not accept `{value}`"
                )));
            }
        }
    }
    Ok(out)
}

fn help_for(args: Vec<String>) -> ParseAction {
    match args.as_slice() {
        [] => ParseAction::help(HELP),
        [flag] if is_help(flag) => ParseAction::help(HELP),
        [topic] => match topic.as_str() {
            "send" => ParseAction::help(SEND_HELP),
            "watch" => ParseAction::help(WATCH_HELP),
            "queue" => ParseAction::help(QUEUE_HELP),
            "web" => ParseAction::help(WEB_HELP),
            "dav" => ParseAction::help(DAV_HELP),
            "socks5" => ParseAction::help(SOCKS5_HELP),
            "http" => ParseAction::help(HTTP_HELP),
            "paste" => ParseAction::help(PASTE_HELP),
            "drop" => ParseAction::help(DROP_HELP),
            "proxy" => ParseAction::help(PROXY_HELP),
            "tcp" => ParseAction::help(TCP_HELP),
            "udp" => ParseAction::help(UDP_HELP),
            "ping" => ParseAction::help(PING_HELP),
            "speed" => ParseAction::help(SPEED_HELP),
            "wake" => ParseAction::help(WAKE_HELP),
            "port" => ParseAction::help(PORT_HELP),
            "health" => ParseAction::help(HEALTH_HELP),
            "pac" => ParseAction::help(PAC_HELP),
            "webrtc" => ParseAction::help(WEBRTC_HELP),
            "tunnel" => ParseAction::help(TUNNEL_HELP),
            "recv" => ParseAction::help(RECV_HELP),
            "relay" => ParseAction::help(RELAY_HELP),
            "discover" => ParseAction::help(DISCOVER_HELP),
            "doctor" => ParseAction::help(DOCTOR_HELP),
            "version" => ParseAction::help(VERSION_HELP),
            _ => ParseAction::error(format!("unknown help topic `{topic}`")),
        },
        _ => ParseAction::error("help accepts at most one command"),
    }
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
    fn bare_web_tokens_are_random_and_preserve_following_options() {
        let send = Cli::parse_from([
            "ii", "send", "file.txt", "--web", "--token", "--port", "45123",
        ]);
        match send.command {
            Command::Send(args) => {
                assert_eq!(args.web_port, Some(45123));
                assert!(is_valid_web_token(args.web_token.as_deref().unwrap()));
                assert_eq!(args.web_token.as_deref().unwrap().len(), 32);
            }
            _ => panic!("expected send command"),
        }

        let web = Cli::parse_from(["ii", "web", "shared", "--token", "--path", "uploads"]);
        match web.command {
            Command::Web(args) => {
                assert_eq!(args.dir, Some(PathBuf::from("shared")));
                assert_eq!(args.web_upload_dir, Some(PathBuf::from("uploads")));
                assert!(is_valid_web_token(args.web_token.as_deref().unwrap()));
                assert_eq!(args.web_token.as_deref().unwrap().len(), 32);
            }
            _ => panic!("expected web command"),
        }

        let webrtc = Cli::parse_from(["ii", "webrtc", "--token"]);
        match webrtc.command {
            Command::Webrtc(args) => {
                assert!(is_valid_web_token(args.web_token.as_deref().unwrap()));
                assert_eq!(args.web_token.as_deref().unwrap().len(), 32);
            }
            _ => panic!("expected webrtc command"),
        }
    }

    #[test]
    fn web_upload_is_explicit_and_path_without_it_is_ignored() {
        for args in [
            vec![
                "ii", "send", "file.txt", "--web", "--upload", "--path", "uploads",
            ],
            vec![
                "ii",
                "send",
                "file.txt",
                "--web",
                "--upload",
                "--path=uploads",
            ],
        ] {
            let cli = Cli::parse_from(args);
            match cli.command {
                Command::Send(args) => {
                    assert!(args.web_upload);
                    assert_eq!(args.web_upload_dir, Some(PathBuf::from("uploads")));
                }
                _ => panic!("expected send command"),
            }
        }

        let cli = Cli::parse_from(["ii", "send", "file.txt", "--web", "--path", "uploads"]);
        match cli.command {
            Command::Send(args) => {
                assert!(!args.web_upload);
                assert_eq!(args.web_upload_dir, Some(PathBuf::from("uploads")));
            }
            _ => panic!("expected send command"),
        }

        let cli = Cli::parse_from(["ii", "web", "shared", "--upload"]);
        match cli.command {
            Command::Web(args) => assert!(args.web_upload),
            _ => panic!("expected web command"),
        }
    }

    #[test]
    fn lan_web_commands_accept_explicit_ports() {
        for args in [
            vec!["ii", "send", "file.txt", "--web", "--port", "45123"],
            vec!["ii", "send", "file.txt", "--web", "--port=45123"],
        ] {
            let cli = Cli::parse_from(args);
            match cli.command {
                Command::Send(args) => assert_eq!(args.web_port, Some(45123)),
                _ => panic!("expected send command"),
            }
        }

        for args in [
            vec!["ii", "web", "shared", "--port", "45123"],
            vec!["ii", "web", "shared", "--port=45123"],
        ] {
            let cli = Cli::parse_from(args);
            match cli.command {
                Command::Web(args) => assert_eq!(args.web_port, Some(45123)),
                _ => panic!("expected web command"),
            }
        }

        for args in [
            vec!["ii", "webrtc", "--port", "45123"],
            vec!["ii", "webrtc", "--port=45123"],
        ] {
            let cli = Cli::parse_from(args);
            match cli.command {
                Command::Webrtc(args) => assert_eq!(args.web_port, Some(45123)),
                _ => panic!("expected webrtc command"),
            }
        }
    }

    #[test]
    fn web_accepts_directory_token_upload_path_and_port() {
        let token = "A1b2C3d4E5f6G7h8";
        for args in [
            vec!["ii", "web"],
            vec![
                "ii", "web", "shared", "--port", "45123", "--token", token, "--upload", "--path",
                "uploads",
            ],
            vec![
                "ii",
                "web",
                "shared",
                "--port=45123",
                "--token=A1b2C3d4E5f6G7h8",
                "--upload",
                "--path=uploads",
            ],
        ] {
            let cli = Cli::parse_from(args);
            match cli.command {
                Command::Web(args) => {
                    if args.dir.is_some() {
                        assert_eq!(args.dir, Some(PathBuf::from("shared")));
                        assert_eq!(args.web_port, Some(45123));
                        assert_eq!(args.web_token.as_deref(), Some(token));
                        assert!(args.web_upload);
                        assert_eq!(args.web_upload_dir, Some(PathBuf::from("uploads")));
                    }
                }
                _ => panic!("expected web command"),
            }
        }
    }

    #[test]
    fn web_once_is_only_for_non_upload_services() {
        let cli = Cli::parse_from(["ii", "web", "shared", "--once"]);
        match cli.command {
            Command::Web(args) => assert!(args.once),
            _ => panic!("expected web command"),
        }
        assert!(matches!(
            parse_args(["ii", "web", "--once", "--upload"]),
            Err(ParseAction::Print { code: 2, .. })
        ));
    }

    #[test]
    fn web_rejects_invalid_options_and_multiple_directories() {
        for args in [
            vec!["ii", "web", "first", "second"],
            vec!["ii", "web", "-p"],
            vec!["ii", "web", "--token", "too-short"],
            vec!["ii", "web", "--token", "A1b2C3d4E5f6G7h!"],
            vec!["ii", "web", "--token="],
            vec!["ii", "web", "--upload=uploads"],
            vec!["ii", "web", "--upload", "--upload"],
            vec!["ii", "web", "--path"],
            vec!["ii", "web", "--port"],
            vec!["ii", "web", "--port=0"],
            vec!["ii", "web", "--port=-1"],
            vec!["ii", "web", "--port=65536"],
            vec!["ii", "web", "--port=nope"],
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
        for args in [
            vec!["ii", "send", "file.txt", "--path", "uploads"],
            vec!["ii", "send", "file.txt", "--upload"],
            vec!["ii", "send", "file.txt", "--upload=uploads"],
            vec!["ii", "send", "file.txt", "--port", "45123"],
            vec!["ii", "send", "file.txt", "--web", "--port=0"],
            vec!["ii", "send", "file.txt", "--web", "--port=-1"],
            vec!["ii", "send", "file.txt", "--web", "--port=65536"],
            vec!["ii", "send", "file.txt", "--web", "--port=nope"],
            vec!["ii", "send", "file.txt", "--web", "--port"],
        ] {
            assert!(matches!(
                parse_args(args),
                Err(ParseAction::Print { code: 2, .. })
            ));
        }
    }

    #[test]
    fn webrtc_accepts_optional_token_and_port() {
        let token = "A1b2C3d4E5f6G7h8";
        for args in [
            vec!["ii", "webrtc"],
            vec!["ii", "webrtc", "--port", "45123", "--token", token],
            vec!["ii", "webrtc", "--port=45123", "--token=A1b2C3d4E5f6G7h8"],
        ] {
            let cli = Cli::parse_from(args);
            match cli.command {
                Command::Webrtc(args) => {
                    if args.web_token.is_some() {
                        assert_eq!(args.web_port, Some(45123));
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
            vec!["ii", "webrtc", "--port"],
            vec!["ii", "webrtc", "--port=0"],
            vec!["ii", "webrtc", "--port=-1"],
            vec!["ii", "webrtc", "--port=65536"],
            vec!["ii", "webrtc", "--port=nope"],
            vec!["ii", "webrtc", "--token", "too-short"],
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
    fn help_command_outputs_root_or_command_help() {
        for args in [
            vec!["ii", "help"],
            vec!["ii", "help", "-h"],
            vec!["ii", "help", "--help"],
        ] {
            match parse_args(args) {
                Err(ParseAction::Print { text, code }) => {
                    assert_eq!(code, 0);
                    assert_eq!(text, HELP);
                }
                Ok(_) => panic!("expected root help"),
            }
        }

        for (topic, expected) in [
            ("send", SEND_HELP),
            ("watch", WATCH_HELP),
            ("queue", QUEUE_HELP),
            ("web", WEB_HELP),
            ("webrtc", WEBRTC_HELP),
            ("tunnel", TUNNEL_HELP),
            ("recv", RECV_HELP),
            ("relay", RELAY_HELP),
            ("doctor", DOCTOR_HELP),
            ("version", VERSION_HELP),
        ] {
            match parse_args(["ii", "help", topic]) {
                Err(ParseAction::Print { text, code }) => {
                    assert_eq!(code, 0, "{topic}");
                    assert_eq!(text, expected, "{topic}");
                }
                Ok(_) => panic!("expected {topic} help"),
            }
        }
    }

    #[test]
    fn help_command_rejects_unknown_or_extra_topics() {
        for args in [
            vec!["ii", "help", "help"],
            vec!["ii", "help", "unknown"],
            vec!["ii", "help", "send", "extra"],
            vec!["ii", "help", "send", "--help"],
        ] {
            match parse_args(args) {
                Err(ParseAction::Print { text, code }) => {
                    assert_eq!(code, 2);
                    assert!(text.starts_with("error: "));
                    assert!(text.ends_with(HELP));
                }
                Ok(_) => panic!("expected help topic error"),
            }
        }
    }

    #[test]
    fn conflicting_backends_keep_exit_code_and_error_text() {
        match parse_args(["ii", "send", "--ftp", "--sftp", "file.txt"]) {
            Err(ParseAction::Print { text, code }) => {
                assert_eq!(code, 2);
                assert!(text.starts_with("error: --s3, --r2, --azure, --webdav, --ftp, --sftp, --web, --local, --relay and --no-relay conflict with each other\n\n"));
                assert!(text.ends_with(HELP));
            }
            Ok(_) => panic!("expected conflicting backends to fail"),
        }
    }

    #[test]
    fn command_help_contracts_keep_existing_text() {
        for (command, expected) in [
            ("send", SEND_HELP),
            ("watch", WATCH_HELP),
            ("queue", QUEUE_HELP),
            ("web", WEB_HELP),
            ("webrtc", WEBRTC_HELP),
            ("tunnel", TUNNEL_HELP),
            ("recv", RECV_HELP),
            ("relay", RELAY_HELP),
            ("doctor", DOCTOR_HELP),
            ("version", VERSION_HELP),
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
    fn send_accepts_r2_and_azure_profiles() {
        for args in [
            vec!["ii", "send", "--r2", "--profile", "work", "-d", "file.txt"],
            vec![
                "ii",
                "send",
                "--azure",
                "--profile",
                "work",
                "-d",
                "file.txt",
            ],
        ] {
            let cli = Cli::parse_from(args);
            match cli.command {
                Command::Send(args) => {
                    assert!(args.r2 || args.azure);
                    assert_eq!(args.profile.as_deref(), Some("work"));
                    assert!(args.delete_after_recv);
                }
                _ => panic!("expected send command"),
            }
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

        let result = parse_args([
            "ii",
            "send",
            "file.txt",
            "--relay",
            "http://127.0.0.1",
            "-k",
        ]);
        assert!(matches!(result, Err(ParseAction::Print { code: 2, .. })));
    }

    #[test]
    fn relay_accepts_default_http_mode_and_port() {
        let cli = Cli::parse_from(["ii", "relay", "--port=8443"]);
        match cli.command {
            Command::Relay(args) => {
                assert_eq!(args.port, Some(8443));
                assert!(!args.tls);
                assert!(args.domain.is_none());
            }
            _ => panic!("expected relay command"),
        }
    }

    #[test]
    fn relay_accepts_self_signed_and_manual_tls_modes() {
        let self_signed =
            Cli::parse_from(["ii", "relay", "--tls", "--domain", "relay.example.com"]);
        match self_signed.command {
            Command::Relay(args) => {
                assert!(args.tls);
                assert_eq!(args.domain.as_deref(), Some("relay.example.com"));
                assert!(args.cert.is_none());
                assert!(args.key.is_none());
            }
            _ => panic!("expected relay command"),
        }

        let cli = Cli::parse_from([
            "ii",
            "relay",
            "--tls",
            "--cert",
            "fullchain.pem",
            "--key",
            "privkey.pem",
            "--port",
            "8443",
        ]);
        match cli.command {
            Command::Relay(args) => {
                assert!(args.tls);
                assert!(args.domain.is_none());
                assert_eq!(args.cert, Some(PathBuf::from("fullchain.pem")));
                assert_eq!(args.key, Some(PathBuf::from("privkey.pem")));
                assert_eq!(args.port, Some(8443));
            }
            _ => panic!("expected relay command"),
        }
    }

    #[test]
    fn relay_rejects_invalid_tls_combinations_and_legacy_flags() {
        for args in [
            vec!["ii", "relay", "--port", "0"],
            vec!["ii", "relay", "--domain", "relay.example.com"],
            vec!["ii", "relay", "--cert", "fullchain.pem"],
            vec!["ii", "relay", "--tls", "--cert", "fullchain.pem"],
            vec!["ii", "relay", "--tls=relay.example.com"],
            vec!["ii", "relay", "--public", "https://relay.example.com"],
            vec!["ii", "relay", "-H", "8443"],
        ] {
            let result = parse_args(args);
            assert!(matches!(result, Err(ParseAction::Print { code: 2, .. })));
        }
    }

    #[test]
    fn send_accepts_http_custom_relay() {
        let cli = Cli::parse_from(["ii", "send", "file.txt", "--relay", "http://127.0.0.1:3340"]);
        match cli.command {
            Command::Send(args) => {
                assert_eq!(args.relay[0].as_str(), "http://127.0.0.1:3340/")
            }
            _ => panic!("expected send command"),
        }
    }

    #[test]
    fn send_accepts_multiple_distinct_relays_and_deduplicates() {
        let cli = Cli::parse_from([
            "ii",
            "send",
            "file.txt",
            "--relay",
            "https://relay-a.example",
            "--relay=https://relay-b.example",
            "--relay",
            "https://relay-a.example",
        ]);
        match cli.command {
            Command::Send(args) => {
                assert_eq!(args.relay.len(), 2);
                assert_eq!(args.relay[0].as_str(), "https://relay-a.example/");
                assert_eq!(args.relay[1].as_str(), "https://relay-b.example/");
            }
            _ => panic!("expected send command"),
        }
    }

    #[test]
    fn socks5_accepts_listener_and_credentials() {
        let cli = Cli::parse_from([
            "ii",
            "socks5",
            "--port=1080",
            "--bind",
            "127.0.0.1",
            "--username",
            "alice",
            "--password=secret",
        ]);
        match cli.command {
            Command::Socks5(args) => {
                assert_eq!(args.port, Some(1080));
                assert_eq!(args.bind, Some("127.0.0.1".parse().unwrap()));
                assert_eq!(args.username.as_deref(), Some("alice"));
                assert_eq!(args.password.as_deref(), Some("secret"));
            }
            _ => panic!("expected socks5 command"),
        }
    }

    #[test]
    fn send_accepts_multiple_paths_filters_rate_and_json() {
        let cli = Cli::parse_from([
            "ii",
            "send",
            "one",
            "two",
            "--include",
            "**/*.rs",
            "--exclude",
            "target/**",
            "--rate",
            "2MiB",
            "--json",
        ]);
        match cli.command {
            Command::Send(args) => {
                assert_eq!(args.extra_paths, [PathBuf::from("two")]);
                assert_eq!(args.include, ["**/*.rs"]);
                assert_eq!(args.exclude, ["target/**"]);
                assert_eq!(args.rate, Some(2 * 1024 * 1024));
                assert!(args.json);
            }
            _ => panic!("expected send command"),
        }
    }

    #[test]
    fn send_accepts_checksum_metadata_symlink_and_quic_port() {
        let cli = Cli::parse_from([
            "ii",
            "send",
            "file.txt",
            "--checksum=sha256",
            "--preserve-metadata",
            "--symlinks",
            "preserve",
            "--quic-port=45123",
        ]);
        match cli.command {
            Command::Send(args) => {
                assert_eq!(args.checksum, Some(ChecksumAlgorithm::Sha256));
                assert!(args.preserve_metadata);
                assert_eq!(args.symlinks, SymlinkPolicy::Preserve);
                assert_eq!(args.quic_port, Some(45123));
            }
            _ => panic!("expected send command"),
        }
    }

    #[test]
    fn watch_and_queue_parse_schedules_and_reject_ambiguous_options() {
        let cli = Cli::parse_from([
            "ii",
            "watch",
            "incoming",
            "--interval=500ms",
            "--stabilize",
            "1s",
        ]);
        match cli.command {
            Command::Watch(args) => {
                assert_eq!(args.dir, PathBuf::from("incoming"));
                assert_eq!(args.interval, std::time::Duration::from_millis(500));
                assert_eq!(args.stabilize, std::time::Duration::from_secs(1));
            }
            _ => panic!("expected watch command"),
        }

        let cli = Cli::parse_from(["ii", "queue", "a", "b", "--every", "2s"]);
        match cli.command {
            Command::Queue(args) => {
                assert_eq!(args.paths, [PathBuf::from("a"), PathBuf::from("b")]);
                assert_eq!(args.every, Some(std::time::Duration::from_secs(2)));
            }
            _ => panic!("expected queue command"),
        }
        assert!(matches!(
            parse_args(["ii", "queue", "a", "--after", "1s", "--every", "2s"]),
            Err(ParseAction::Print { code: 2, .. })
        ));
    }

    #[test]
    fn rate_parser_rejects_zero_and_overflow() {
        assert!(matches!(parse_rate("--rate", "1"), Ok(1)));
        assert!(matches!(parse_rate("--rate", "4KiB"), Ok(4096)));
        assert!(parse_rate("--rate", "0").is_err());
        assert!(parse_rate("--rate", "18446744073709551615GiB").is_err());
    }

    #[test]
    fn duration_parser_requires_a_positive_supported_unit() {
        assert_eq!(
            parse_duration("--after", "500ms").ok(),
            Some(std::time::Duration::from_millis(500))
        );
        assert_eq!(
            parse_duration("--after", "2s").ok(),
            Some(std::time::Duration::from_secs(2))
        );
        for value in ["0s", "1d", "2", "-1s", "999999999999999999999h"] {
            assert!(parse_duration("--after", value).is_err(), "{value}");
        }
    }

    #[test]
    fn doctor_accepts_only_the_nat_probe_flag() {
        match Cli::parse_from(["ii", "doctor", "--nat"]).command {
            Command::Doctor(args) => assert!(args.nat),
            _ => panic!("expected doctor command"),
        }
        assert!(matches!(
            parse_args(["ii", "doctor", "--nat", "--nat"]),
            Err(ParseAction::Print { code: 2, .. })
        ));
    }

    #[test]
    fn discover_and_dav_parse_their_options() {
        assert!(matches!(
            Cli::parse_from(["ii", "discover", "--json"]).command,
            Command::Discover(DiscoverArgs { json: true })
        ));
        match Cli::parse_from([
            "ii",
            "dav",
            "share",
            "--bind",
            "::1",
            "--read-only",
            "--token",
            "A1b2C3d4E5f6G7h8",
        ])
        .command
        {
            Command::Dav(args) => {
                assert_eq!(args.dir, Some(PathBuf::from("share")));
                assert_eq!(args.web_bind, Some("::1".parse().unwrap()));
                assert!(args.read_only);
                assert!(args.web_token.is_some());
            }
            _ => panic!("expected dav command"),
        }
    }

    #[test]
    fn dav_accepts_basic_auth_and_tls_options() {
        let cli = Cli::parse_from([
            "ii",
            "dav",
            "share",
            "--username",
            "alice",
            "--password=secret",
            "--tls",
            "--domain",
            "dav.example.com",
            "--cert",
            "fullchain.pem",
            "--key",
            "privkey.pem",
        ]);
        match cli.command {
            Command::Dav(args) => {
                assert_eq!(args.username.as_deref(), Some("alice"));
                assert_eq!(args.password.as_deref(), Some("secret"));
                assert!(args.tls);
                assert_eq!(args.domain.as_deref(), Some("dav.example.com"));
                assert_eq!(args.cert, Some(PathBuf::from("fullchain.pem")));
                assert_eq!(args.key, Some(PathBuf::from("privkey.pem")));
            }
            _ => panic!("expected dav command"),
        }

        for args in [
            vec!["ii", "dav", "--username", "alice"],
            vec!["ii", "dav", "--password", "secret"],
            vec!["ii", "dav", "--domain", "dav.example.com"],
            vec!["ii", "dav", "--tls", "--cert", "fullchain.pem"],
            vec![
                "ii",
                "dav",
                "--tls",
                "--username",
                "",
                "--password",
                "secret",
            ],
        ] {
            assert!(parse_args(args).is_err());
        }
    }
}
