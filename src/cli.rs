use std::fmt;
use std::path::PathBuf;
use std::process;
use std::str::FromStr;

#[derive(Debug)]
pub struct Cli {
    pub command: Command,
}

#[derive(Debug)]
pub enum Command {
    Send(SendArgs),
    Recv(RecvArgs),
    Relay(RelayArgs),
    Doctor,
    Version,
}

#[derive(Debug, Clone, Default)]
pub struct SendArgs {
    pub path: Option<PathBuf>,
    pub name: Option<String>,
    pub keep_alive: bool,
    pub copy: bool,
    pub output: Option<PathBuf>,
    pub s3: bool,
    pub delete_after_recv: bool,
    pub profile: Option<String>,
    pub webdav: bool,
    pub portable_webdav: bool,
    pub local: bool,
    pub relay: Option<iroh::RelayUrl>,
    pub no_relay: bool,
}

#[derive(Debug, Clone)]
pub struct RecvArgs {
    pub ticket: String,
    pub out_dir: Option<PathBuf>,
    pub stdout: bool,
    pub overwrite: bool,
    pub resume: bool,
    pub local: bool,
    pub trace: bool,
}

#[derive(Debug, Clone, Default)]
pub struct RelayArgs {
    pub config: Option<PathBuf>,
    pub port: Option<u16>,
    pub metrics: Option<u16>,
    pub tls_domain: Option<String>,
    pub cert: Option<PathBuf>,
    pub key: Option<PathBuf>,
}

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

enum ParseAction {
    Print { text: String, code: i32 },
}

impl ParseAction {
    fn error(message: impl fmt::Display) -> Self {
        Self::Print {
            text: format!("error: {message}\n\n{}", HELP),
            code: 2,
        }
    }

    fn help(text: &'static str) -> Self {
        Self::Print {
            text: text.to_string(),
            code: 0,
        }
    }

    fn version() -> Self {
        Self::Print {
            text: env!("CARGO_PKG_VERSION").to_string(),
            code: 0,
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
        "send" => Command::Send(parse_send(rest)?),
        "recv" => Command::Recv(parse_recv(rest)?),
        "relay" => Command::Relay(parse_relay(rest)?),
        "doctor" => reject_extra("doctor", rest).map(|_| Command::Doctor)?,
        "version" => reject_extra("version", rest).map(|_| Command::Version)?,
        other => return Err(ParseAction::error(format!("unknown command `{other}`"))),
    };

    Ok(Cli { command })
}

fn parse_send(args: Vec<String>) -> Result<SendArgs, ParseAction> {
    let mut out = SendArgs::default();
    let mut iter = ArgsIter::new(args);

    while let Some(arg) = iter.next() {
        match split_long_value(&arg) {
            Some(("name", value)) => out.name = Some(value.to_string()),
            Some(("output", value)) => out.output = Some(PathBuf::from(value)),
            Some(("profile", value)) => out.profile = Some(value.to_string()),
            Some(("relay", value)) => out.relay = Some(parse_relay_url(value)?),
            Some((flag, _)) => {
                return Err(ParseAction::error(format!("unknown option `--{flag}`")));
            }
            None => match arg.as_str() {
                "-h" | "--help" => return Err(ParseAction::help(SEND_HELP)),
                "--name" => out.name = Some(iter.value("--name")?),
                "-t" => out.keep_alive = true,
                "-c" | "--copy" => out.copy = true,
                "-o" | "--output" => out.output = Some(PathBuf::from(iter.value(&arg)?)),
                "--s3" => out.s3 = true,
                "-d" => out.delete_after_recv = true,
                "--profile" => out.profile = Some(iter.value("--profile")?),
                "--webdav" => out.webdav = true,
                "-p" => out.portable_webdav = true,
                "--local" => out.local = true,
                "--relay" => out.relay = Some(parse_relay_url(&iter.value("--relay")?)?),
                "--no-relay" => out.no_relay = true,
                _ if arg.starts_with('-') => {
                    return Err(ParseAction::error(format!("unknown option `{arg}`")));
                }
                _ => {
                    if out.path.replace(PathBuf::from(&arg)).is_some() {
                        return Err(ParseAction::error("send accepts only one path"));
                    }
                }
            },
        }
    }

    validate_send(&out)?;
    Ok(out)
}

fn parse_recv(args: Vec<String>) -> Result<RecvArgs, ParseAction> {
    let mut ticket = None;
    let mut out_dir = None;
    let mut stdout = false;
    let mut overwrite = false;
    let mut resume = false;
    let mut local = false;
    let mut trace = false;
    let mut iter = ArgsIter::new(args);

    while let Some(arg) = iter.next() {
        match split_long_value(&arg) {
            Some(("output", value)) => out_dir = Some(PathBuf::from(value)),
            Some((flag, _)) => {
                return Err(ParseAction::error(format!("unknown option `--{flag}`")));
            }
            None => match arg.as_str() {
                "-h" | "--help" => return Err(ParseAction::help(RECV_HELP)),
                "-o" => out_dir = Some(PathBuf::from(iter.value("-o")?)),
                "--stdout" => stdout = true,
                "--overwrite" => overwrite = true,
                "--resume" => resume = true,
                "--local" => local = true,
                "--trace" => trace = true,
                _ if arg.starts_with('-') => {
                    return Err(ParseAction::error(format!("unknown option `{arg}`")));
                }
                _ => {
                    if ticket.replace(arg).is_some() {
                        return Err(ParseAction::error("recv accepts only one ticket"));
                    }
                }
            },
        }
    }

    if stdout && resume {
        return Err(ParseAction::error("--stdout conflicts with --resume"));
    }

    let Some(ticket) = ticket else {
        return Err(ParseAction::error("missing ticket"));
    };

    Ok(RecvArgs {
        ticket,
        out_dir,
        stdout,
        overwrite,
        resume,
        local,
        trace,
    })
}

fn parse_relay(args: Vec<String>) -> Result<RelayArgs, ParseAction> {
    let mut out = RelayArgs::default();
    let mut iter = ArgsIter::new(args);

    while let Some(arg) = iter.next() {
        match split_long_value(&arg) {
            Some(("config", value)) => out.config = Some(PathBuf::from(value)),
            Some(("port", value)) | Some(("http", value)) => {
                out.port = Some(parse_port("--port", value)?)
            }
            Some(("metrics", value)) => out.metrics = Some(parse_port("--metrics", value)?),
            Some(("tls", value)) => out.tls_domain = Some(value.to_string()),
            Some(("cert", value)) => out.cert = Some(PathBuf::from(value)),
            Some(("key", value)) => out.key = Some(PathBuf::from(value)),
            Some((flag, _)) => {
                return Err(ParseAction::error(format!("unknown option `--{flag}`")));
            }
            None => match arg.as_str() {
                "-h" | "--help" => return Err(ParseAction::help(RELAY_HELP)),
                "-c" | "--config" => out.config = Some(PathBuf::from(iter.value(&arg)?)),
                "-H" | "--port" | "--http" => {
                    out.port = Some(parse_port(&arg, &iter.value(&arg)?)?)
                }
                "-M" | "--metrics" => out.metrics = Some(parse_port(&arg, &iter.value(&arg)?)?),
                "--tls" => out.tls_domain = Some(iter.value("--tls")?),
                "--cert" => out.cert = Some(PathBuf::from(iter.value("--cert")?)),
                "--key" => out.key = Some(PathBuf::from(iter.value("--key")?)),
                _ => return Err(ParseAction::error(format!("unexpected argument `{arg}`"))),
            },
        }
    }

    validate_relay(&out)?;
    Ok(out)
}

fn validate_send(args: &SendArgs) -> Result<(), ParseAction> {
    let backend_count = [
        args.s3,
        args.webdav,
        args.local,
        args.relay.is_some(),
        args.no_relay,
    ]
    .into_iter()
    .filter(|value| *value)
    .count();

    if backend_count > 1 {
        return Err(ParseAction::error(
            "--s3, --webdav, --local, --relay and --no-relay conflict with each other",
        ));
    }

    if args.portable_webdav && !args.webdav {
        return Err(ParseAction::error("-p requires --webdav"));
    }

    Ok(())
}

fn validate_relay(args: &RelayArgs) -> Result<(), ParseAction> {
    if let Some(domain) = &args.tls_domain {
        if domain.is_empty()
            || domain.contains("://")
            || domain.contains('/')
            || domain.contains(':')
        {
            return Err(ParseAction::error(
                "--tls expects a bare DNS name such as relay.example.com",
            ));
        }
    }

    match (&args.tls_domain, &args.cert, &args.key) {
        (Some(_), Some(_), Some(_)) | (None, None, None) => Ok(()),
        (Some(_), _, _) => Err(ParseAction::error(
            "--tls requires both --cert <path> and --key <path>",
        )),
        (None, _, _) => Err(ParseAction::error(
            "--cert and --key require --tls <domain>",
        )),
    }
}

fn reject_extra(command: &str, args: Vec<String>) -> Result<(), ParseAction> {
    if args.iter().any(|arg| is_help(arg)) {
        return Err(ParseAction::help(match command {
            "doctor" => DOCTOR_HELP,
            "version" => VERSION_HELP,
            _ => HELP,
        }));
    }
    if let Some(extra) = args.first() {
        return Err(ParseAction::error(format!(
            "`{command}` does not accept `{extra}`"
        )));
    }
    Ok(())
}

fn parse_relay_url(value: &str) -> Result<iroh::RelayUrl, ParseAction> {
    iroh::RelayUrl::from_str(value)
        .map_err(|err| ParseAction::error(format!("invalid relay URL `{value}`: {err}")))
}

fn parse_port(flag: &str, value: &str) -> Result<u16, ParseAction> {
    value
        .parse()
        .map_err(|_| ParseAction::error(format!("{flag} expects a port from 0 to 65535")))
}

fn split_long_value(arg: &str) -> Option<(&str, &str)> {
    arg.strip_prefix("--")?.split_once('=')
}

fn is_help(arg: &str) -> bool {
    arg == "-h" || arg == "--help"
}

struct ArgsIter {
    args: std::vec::IntoIter<String>,
}

impl ArgsIter {
    fn new(args: Vec<String>) -> Self {
        Self {
            args: args.into_iter(),
        }
    }

    fn next(&mut self) -> Option<String> {
        self.args.next()
    }

    fn value(&mut self, flag: &str) -> Result<String, ParseAction> {
        self.next()
            .ok_or_else(|| ParseAction::error(format!("{flag} expects a value")))
    }
}

const HELP: &str = "\
ii file transfer

Usage:
  ii send [options] [path]
  ii recv [options] <ticket>
  ii relay [options]
  ii doctor
  ii version
";

const SEND_HELP: &str = "\
Usage:
  ii send [options] [path]

Options:
  --name <name>
  -t
  -c, --copy
  -o, --output <path>
  --s3
  --webdav
  -p
  -d
  --profile <name>
  --local
  --relay <url>
  --no-relay
";

const RECV_HELP: &str = "\
Usage:
  ii recv [options] <ticket>

Options:
  -o <dir>
  --stdout
  --overwrite
  --resume
  --local
  --trace
";

const RELAY_HELP: &str = "\
Usage:
  ii relay [options]

Options:
  -c, --config <path>
  -H, --port <port>
  -M, --metrics <port>
  --tls <domain>
  --cert <path>
  --key <path>
";

const DOCTOR_HELP: &str = "Usage:\n  ii doctor";
const VERSION_HELP: &str = "Usage:\n  ii version";

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
    fn relay_accepts_tls_with_manual_certificate_paths() {
        let cli = Cli::parse_from([
            "ii",
            "relay",
            "--tls",
            "relay.example.com",
            "-H",
            "8443",
            "--cert",
            "fullchain.pem",
            "--key",
            "privkey.pem",
        ]);
        match cli.command {
            Command::Relay(args) => {
                assert_eq!(args.port, Some(8443));
                assert_eq!(args.tls_domain.as_deref(), Some("relay.example.com"));
                assert_eq!(args.cert, Some(PathBuf::from("fullchain.pem")));
                assert_eq!(args.key, Some(PathBuf::from("privkey.pem")));
            }
            _ => panic!("expected relay command"),
        }
    }

    #[test]
    fn relay_rejects_incomplete_tls_arguments() {
        let result = parse_args(["ii", "relay", "--tls", "relay.example.com"]);
        assert!(matches!(result, Err(ParseAction::Print { code: 2, .. })));
    }

    #[test]
    fn relay_rejects_tls_url_instead_of_domain() {
        let result = parse_args([
            "ii",
            "relay",
            "--tls",
            "https://relay.example.com",
            "--cert",
            "fullchain.pem",
            "--key",
            "privkey.pem",
        ]);
        assert!(matches!(result, Err(ParseAction::Print { code: 2, .. })));
    }

    #[test]
    fn relay_rejects_removed_dev_mode() {
        let result = parse_args(["ii", "relay", "--dev"]);
        assert!(matches!(result, Err(ParseAction::Print { code: 2, .. })));
    }
}
