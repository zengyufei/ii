use super::*;
use crate::command::FtpArgs;
use std::{
    net::{IpAddr, Ipv4Addr},
    ops::RangeInclusive,
    path::PathBuf,
};

const DEFAULT_PORT: u16 = 21;
const DEFAULT_IMPLICIT_TLS_PORT: u16 = 990;
const DEFAULT_MAX_CONNECTIONS: usize = 100;
const DEFAULT_PASSIVE_PORTS: RangeInclusive<u16> = 49152..=65535;

pub(super) fn parse(args: Vec<String>) -> Result<FtpArgs, ParseAction> {
    let mut out = FtpArgs {
        dir: None,
        port: DEFAULT_PORT,
        bind: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        username: None,
        password: None,
        rate: None,
        max_connections: DEFAULT_MAX_CONNECTIONS,
        upload: true,
        download: true,
        delete: true,
        rename: true,
        mkdir: true,
        tls: false,
        implicit_tls: false,
        cert: None,
        key: None,
        passive_host: None,
        passive_ports: None,
    };
    let mut port_was_set = false;
    let mut iter = ArgsIter::new(args);
    while let Some(arg) = iter.next() {
        if is_help(&arg) {
            return Err(ParseAction::help(FTP_HELP));
        }
        match split_long_value(&arg) {
            Some(("bind", value)) => out.bind = parse_bind("--bind", value)?,
            Some(("port", value)) => {
                out.port = parse_port("--port", value)?;
                port_was_set = true;
            }
            Some(("username", value)) => out.username = Some(value.to_string()),
            Some(("password", value)) => out.password = Some(value.to_string()),
            Some(("rate", value)) => out.rate = Some(parse_rate("--rate", value)?),
            Some(("max", value)) => out.max_connections = parse_max("--max", value)?,
            Some(("upload", value)) => out.upload = parse_bool("--upload", value)?,
            Some(("download", value)) => out.download = parse_bool("--download", value)?,
            Some(("delete", value)) => out.delete = parse_bool("--delete", value)?,
            Some(("rename", value)) => out.rename = parse_bool("--rename", value)?,
            Some(("mkdir", value)) => out.mkdir = parse_bool("--mkdir", value)?,
            Some(("cert", value)) => out.cert = Some(PathBuf::from(value)),
            Some(("key", value)) => out.key = Some(PathBuf::from(value)),
            Some(("tls", _)) | Some(("implicit", _)) => {
                return Err(ParseAction::error(format!(
                    "--{} does not take a value",
                    arg.trim_start_matches('-')
                        .split('=')
                        .next()
                        .unwrap_or_default()
                )));
            }
            Some(("passive-host", value)) => out.passive_host = Some(parse_passive_host(value)?),
            Some(("passive-ports", value)) => out.passive_ports = Some(parse_passive_ports(value)?),
            Some((flag, _)) => {
                return Err(ParseAction::error(format!("unknown option `--{flag}`")));
            }
            None => match arg.as_str() {
                "--bind" => out.bind = parse_bind("--bind", &iter.value("--bind")?)?,
                "--port" => {
                    out.port = parse_port("--port", &iter.value("--port")?)?;
                    port_was_set = true;
                }
                "--username" => out.username = Some(iter.value("--username")?),
                "--password" => out.password = Some(iter.value("--password")?),
                "--rate" => out.rate = Some(parse_rate("--rate", &iter.value("--rate")?)?),
                "--max" => out.max_connections = parse_max("--max", &iter.value("--max")?)?,
                "--upload" => out.upload = parse_bool("--upload", &iter.value("--upload")?)?,
                "--download" => {
                    out.download = parse_bool("--download", &iter.value("--download")?)?
                }
                "--delete" => out.delete = parse_bool("--delete", &iter.value("--delete")?)?,
                "--rename" => out.rename = parse_bool("--rename", &iter.value("--rename")?)?,
                "--mkdir" => out.mkdir = parse_bool("--mkdir", &iter.value("--mkdir")?)?,
                "--tls" => out.tls = true,
                "--implicit" => out.implicit_tls = true,
                "--cert" => out.cert = Some(PathBuf::from(iter.value("--cert")?)),
                "--key" => out.key = Some(PathBuf::from(iter.value("--key")?)),
                "--passive-host" => {
                    out.passive_host = Some(parse_passive_host(&iter.value("--passive-host")?)?)
                }
                "--passive-ports" => {
                    out.passive_ports = Some(match iter.peek() {
                        Some(value)
                            if !value.starts_with('-') && looks_like_passive_ports(value) =>
                        {
                            parse_passive_ports(&iter.next().expect("peeked passive port range"))?
                        }
                        _ => DEFAULT_PASSIVE_PORTS,
                    })
                }
                value if value.starts_with('-') => {
                    return Err(ParseAction::error(format!("unknown option `{value}`")));
                }
                value => {
                    if out.dir.replace(PathBuf::from(value)).is_some() {
                        return Err(ParseAction::error("ftp accepts only one directory"));
                    }
                }
            },
        }
    }
    if out.username.is_some() != out.password.is_some() {
        return Err(ParseAction::error(
            "--username and --password must be provided together",
        ));
    }
    if out
        .username
        .as_deref()
        .is_some_and(|value| value.is_empty())
        || out
            .password
            .as_deref()
            .is_some_and(|value| value.is_empty())
    {
        return Err(ParseAction::error(
            "--username and --password must not be empty",
        ));
    }
    if out
        .username
        .as_deref()
        .is_some_and(|value| value.len() > u8::MAX as usize)
        || out
            .password
            .as_deref()
            .is_some_and(|value| value.len() > u8::MAX as usize)
    {
        return Err(ParseAction::error(
            "--username and --password must contain at most 255 bytes",
        ));
    }
    if out.passive_host.is_some() && out.passive_ports.is_none() {
        return Err(ParseAction::error(
            "--passive-host requires --passive-ports",
        ));
    }
    if out.implicit_tls && !out.tls {
        return Err(ParseAction::error("--implicit requires --tls"));
    }
    if (out.cert.is_some() || out.key.is_some()) && !out.tls {
        return Err(ParseAction::error("--cert and --key require --tls"));
    }
    if out.cert.is_some() != out.key.is_some() {
        return Err(ParseAction::error(
            "--cert and --key must be provided together",
        ));
    }
    if out.implicit_tls && !port_was_set {
        out.port = DEFAULT_IMPLICIT_TLS_PORT;
    }
    Ok(out)
}

fn parse_max(flag: &str, value: &str) -> Result<usize, ParseAction> {
    let max = value
        .parse::<usize>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| ParseAction::error(format!("{flag} expects a positive integer")))?;
    Ok(max)
}

fn parse_bool(flag: &str, value: &str) -> Result<bool, ParseAction> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(ParseAction::error(format!("{flag} expects true or false"))),
    }
}

fn parse_passive_host(value: &str) -> Result<String, ParseAction> {
    if value.is_empty() || value.bytes().any(|byte| byte.is_ascii_whitespace()) {
        return Err(ParseAction::error(
            "--passive-host expects an IPv4 address or DNS name",
        ));
    }
    if let Ok(ip) = value.parse::<IpAddr>() {
        return ip.is_ipv4().then(|| value.to_string()).ok_or_else(|| {
            ParseAction::error("--passive-host expects an IPv4 address or DNS name")
        });
    }
    if value.len() > 253
        || !value.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label
                    .bytes()
                    .next()
                    .is_some_and(|byte| byte.is_ascii_alphanumeric())
                && label
                    .bytes()
                    .last()
                    .is_some_and(|byte| byte.is_ascii_alphanumeric())
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
    {
        return Err(ParseAction::error(
            "--passive-host expects an IPv4 address or DNS name",
        ));
    }
    Ok(value.to_string())
}

fn looks_like_passive_ports(value: &str) -> bool {
    value.contains('-') || value.bytes().all(|byte| byte.is_ascii_digit())
}

fn parse_passive_ports(value: &str) -> Result<RangeInclusive<u16>, ParseAction> {
    let (first, last) = value.split_once('-').ok_or_else(|| {
        ParseAction::error("--passive-ports expects an inclusive range such as 49152-65535")
    })?;
    let first = first.parse::<u16>().ok().filter(|port| *port > 0);
    let last = last.parse::<u16>().ok().filter(|port| *port > 0);
    let (Some(first), Some(last)) = (first, last) else {
        return Err(ParseAction::error(
            "--passive-ports expects ports from 1 to 65535",
        ));
    };
    if first > last {
        return Err(ParseAction::error(
            "--passive-ports start must not exceed end",
        ));
    }
    Ok(first..=last)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_active_anonymous_service() {
        let args = parse(vec![]).unwrap();
        assert_eq!(args.dir, None);
        assert_eq!(args.bind, IpAddr::V4(Ipv4Addr::UNSPECIFIED));
        assert_eq!(args.port, DEFAULT_PORT);
        assert_eq!(args.max_connections, DEFAULT_MAX_CONNECTIONS);
        assert!(args.upload);
        assert!(args.download);
        assert!(args.delete);
        assert!(args.rename);
        assert!(args.mkdir);
        assert!(!args.tls);
        assert!(!args.implicit_tls);
        assert!(args.cert.is_none());
        assert!(args.key.is_none());
        assert!(args.username.is_none());
        assert!(args.password.is_none());
        assert!(args.passive_host.is_none());
        assert!(args.passive_ports.is_none());
    }

    #[test]
    fn accepts_ftp_options_and_passive_mode() {
        let args = parse(
            [
                "share",
                "--bind=127.0.0.1",
                "--port=2121",
                "--username=alice",
                "--password=secret",
                "--rate=2MiB",
                "--max=4",
                "--upload=false",
                "--download",
                "false",
                "--delete=false",
                "--rename",
                "false",
                "--mkdir=false",
                "--tls",
                "--cert=server.pem",
                "--key",
                "server.key",
                "--passive-host=ftp.example.test",
                "--passive-ports=41000-41020",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
        )
        .unwrap();
        assert_eq!(args.dir, Some(PathBuf::from("share")));
        assert_eq!(args.bind, "127.0.0.1".parse::<IpAddr>().unwrap());
        assert_eq!(args.port, 2121);
        assert_eq!(args.username.as_deref(), Some("alice"));
        assert_eq!(args.password.as_deref(), Some("secret"));
        assert_eq!(args.rate, Some(2 * 1024 * 1024));
        assert_eq!(args.max_connections, 4);
        assert!(!args.upload);
        assert!(!args.download);
        assert!(!args.delete);
        assert!(!args.rename);
        assert!(!args.mkdir);
        assert!(args.tls);
        assert!(!args.implicit_tls);
        assert_eq!(args.cert, Some(PathBuf::from("server.pem")));
        assert_eq!(args.key, Some(PathBuf::from("server.key")));
        assert_eq!(args.passive_host.as_deref(), Some("ftp.example.test"));
        assert_eq!(args.passive_ports, Some(41000..=41020));
    }

    #[test]
    fn bare_passive_ports_use_the_default_range() {
        let args = parse(vec!["share".to_string(), "--passive-ports".to_string()]).unwrap();
        assert_eq!(args.dir, Some(PathBuf::from("share")));
        assert_eq!(args.passive_ports, Some(DEFAULT_PASSIVE_PORTS));
    }

    #[test]
    fn implicit_tls_defaults_to_port_990() {
        let args = parse(vec!["--tls".to_string(), "--implicit".to_string()]).unwrap();
        assert!(args.tls);
        assert!(args.implicit_tls);
        assert_eq!(args.port, DEFAULT_IMPLICIT_TLS_PORT);

        let args = parse(
            ["--tls", "--implicit", "--port", "2121"]
                .into_iter()
                .map(str::to_string)
                .collect(),
        )
        .unwrap();
        assert_eq!(args.port, 2121);
    }

    #[test]
    fn bare_passive_ports_do_not_consume_the_next_option() {
        assert!(matches!(
            parse(vec!["--passive-ports".to_string(), "--help".to_string()]),
            Err(ParseAction::Print { code: 0, .. })
        ));
    }

    #[test]
    fn rejects_invalid_ftp_options() {
        for args in [
            vec!["--username", "alice"],
            vec!["--password", "secret"],
            vec!["--username", "", "--password", "secret"],
            vec!["--username", "alice", "--password", ""],
            vec!["--max", "0"],
            vec!["--upload", "yes"],
            vec!["--passive-host", "127.0.0.1"],
            vec!["--passive-host", "::1", "--passive-ports", "41000-41020"],
            vec![
                "--passive-host",
                "not/a-host",
                "--passive-ports",
                "41000-41020",
            ],
            vec!["--passive-ports", "0-41020"],
            vec!["--passive-ports", "41020-41000"],
            vec!["--implicit"],
            vec!["--cert", "server.pem"],
            vec!["--tls", "--cert", "server.pem"],
            vec!["--tls=implicit"],
            vec!["--tls", "-k"],
            vec!["one", "two"],
        ] {
            assert!(parse(args.into_iter().map(str::to_string).collect()).is_err());
        }
    }
}
