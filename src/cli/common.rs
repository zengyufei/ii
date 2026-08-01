use super::{SendArgs, help::*};
use std::{fmt, net::SocketAddr};

pub(crate) enum ParseAction {
    Print { text: String, code: i32 },
}

impl ParseAction {
    pub(crate) fn error(message: impl fmt::Display) -> Self {
        Self::Print {
            text: format!("error: {message}\n\n{}", HELP),
            code: 2,
        }
    }

    pub(crate) fn help(text: &'static str) -> Self {
        Self::Print {
            text: text.to_string(),
            code: 0,
        }
    }

    pub(crate) fn version() -> Self {
        Self::Print {
            text: env!("CARGO_PKG_VERSION").to_string(),
            code: 0,
        }
    }
}

pub(crate) fn validate_send(args: &SendArgs) -> Result<(), ParseAction> {
    let backend_count = [
        args.s3,
        args.ftp,
        args.webdav,
        args.sftp,
        args.web,
        args.local,
        args.relay.is_some(),
        args.no_relay,
    ]
    .into_iter()
    .filter(|value| *value)
    .count();

    if backend_count > 1 {
        return Err(ParseAction::error(
            "--s3, --webdav, --ftp, --sftp, --web, --local, --relay and --no-relay conflict with each other",
        ));
    }

    if args.portable_webdav && !args.webdav && !args.ftp && !args.sftp {
        return Err(ParseAction::error("-p requires --webdav, --ftp or --sftp"));
    }
    if args.accept_self_signed_relay && args.relay.is_none() {
        return Err(ParseAction::error("-k requires --relay <https-url>"));
    }
    if args.web && args.path.is_none() {
        return Err(ParseAction::error("--web requires a file or folder path"));
    }
    if args.web && (args.copy || args.output.is_some()) {
        return Err(ParseAction::error("--web cannot be used with -c or -o"));
    }
    if args.web_token.is_some() && !args.web {
        return Err(ParseAction::error("--token requires --web"));
    }
    if args.web_upload_dir.is_some() && !args.web {
        return Err(ParseAction::error("--path requires --web"));
    }
    if let Some(token) = args.web_token.as_deref()
        && !is_valid_web_token(token)
    {
        return Err(ParseAction::error(
            "--token must contain 16 to 128 ASCII letters, digits, '-' or '_'",
        ));
    }

    Ok(())
}

pub(crate) fn is_valid_web_token(token: &str) -> bool {
    (16..=128).contains(&token.len())
        && token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

pub(crate) fn reject_extra(command: &str, args: Vec<String>) -> Result<(), ParseAction> {
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

pub(crate) fn parse_relay_url(value: &str) -> Result<iroh::RelayUrl, ParseAction> {
    parse_public_relay_url(value)
}

pub(crate) fn parse_public_relay_url(value: &str) -> Result<iroh::RelayUrl, ParseAction> {
    let url = url::Url::parse(value)
        .map_err(|err| ParseAction::error(format!("invalid relay URL `{value}`: {err}")))?;
    if url.scheme() != "https" {
        return Err(ParseAction::error("relay URL must use https://"));
    }
    if url.host_str().is_none() {
        return Err(ParseAction::error("relay URL must include a host"));
    }
    if !url.username().is_empty()
        || url.password().is_some()
        || !(url.path().is_empty() || url.path() == "/")
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(ParseAction::error(
            "relay URL may contain only https://host[:port]",
        ));
    }
    if url.port() == Some(0) {
        return Err(ParseAction::error("relay URL port must be from 1 to 65535"));
    }
    Ok(iroh::RelayUrl::from(url))
}

pub(crate) fn parse_tls_domain(value: &str) -> Result<String, ParseAction> {
    if value.is_empty()
        || value.contains("://")
        || value.contains('/')
        || value.contains(':')
        || value.parse::<std::net::IpAddr>().is_ok()
    {
        return Err(ParseAction::error(
            "--tls expects a bare DNS name such as relay.example.com",
        ));
    }
    Ok(value.to_string())
}

pub(crate) fn parse_port(flag: &str, value: &str) -> Result<u16, ParseAction> {
    let port: u16 = value
        .parse()
        .map_err(|_| ParseAction::error(format!("{flag} expects a port from 1 to 65535")))?;
    if port == 0 {
        return Err(ParseAction::error(format!(
            "{flag} expects a port from 1 to 65535"
        )));
    }
    Ok(port)
}

pub(crate) fn parse_listen_addr(flag: &str, value: &str) -> Result<SocketAddr, ParseAction> {
    let address: SocketAddr = value
        .parse()
        .map_err(|_| ParseAction::error(format!("{flag} expects an IP address and port")))?;
    if address.port() == 0 {
        return Err(ParseAction::error(format!(
            "{flag} expects a port from 1 to 65535"
        )));
    }
    Ok(address)
}

pub(crate) fn parse_tunnel_target(value: &str) -> Result<String, ParseAction> {
    let (host, port) = if let Some(value) = value.strip_prefix('[') {
        let (host, port) = value
            .split_once("]:")
            .ok_or_else(|| ParseAction::error("-s expects host:port or [IPv6]:port"))?;
        if host.parse::<std::net::Ipv6Addr>().is_err() {
            return Err(ParseAction::error("-s has an invalid IPv6 address"));
        }
        (host, port)
    } else {
        let (host, port) = value
            .rsplit_once(':')
            .ok_or_else(|| ParseAction::error("-s expects host:port or [IPv6]:port"))?;
        if host.contains(':') {
            return Err(ParseAction::error("IPv6 targets must use [address]:port"));
        }
        (host, port)
    };
    if host.is_empty()
        || host
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte == b'/')
    {
        return Err(ParseAction::error("-s has an invalid target host"));
    }
    parse_port("-s", port)?;
    Ok(value.to_string())
}

pub(crate) fn split_long_value(arg: &str) -> Option<(&str, &str)> {
    arg.strip_prefix("--")?.split_once('=')
}

pub(crate) fn is_help(arg: &str) -> bool {
    arg == "-h" || arg == "--help"
}

pub(crate) struct ArgsIter {
    args: std::vec::IntoIter<String>,
}

impl ArgsIter {
    pub(crate) fn new(args: Vec<String>) -> Self {
        Self {
            args: args.into_iter(),
        }
    }

    pub(crate) fn next(&mut self) -> Option<String> {
        self.args.next()
    }

    pub(crate) fn value(&mut self, flag: &str) -> Result<String, ParseAction> {
        self.next()
            .ok_or_else(|| ParseAction::error(format!("{flag} expects a value")))
    }
}
