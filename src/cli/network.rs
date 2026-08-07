use super::*;
use crate::command::{
    DropArgs, ForwardArgs, HealthArgs, HttpArgs, LanHttpArgs, PacArgs, PasteArgs, PingArgs,
    PortArgs, ProxyArgs, SpeedArgs, WakeArgs,
};
use std::{net::IpAddr, path::PathBuf, time::Duration};

fn lan_option(arg: &str, iter: &mut ArgsIter, out: &mut LanHttpArgs) -> Result<bool, ParseAction> {
    match split_long_value(arg) {
        Some(("port", value)) => out.port = Some(parse_port("--port", value)?),
        Some(("bind", value)) => out.bind = Some(parse_bind("--bind", value)?),
        Some(("token", value)) => out.token = Some(value.to_string()),
        Some((_, _)) => return Ok(false),
        None => match arg {
            "--port" => out.port = Some(parse_port("--port", &iter.value("--port")?)?),
            "--bind" => out.bind = Some(parse_bind("--bind", &iter.value("--bind")?)?),
            "--token" => out.token = Some(web_token(iter)),
            _ => return Ok(false),
        },
    }
    Ok(true)
}

fn validate_lan(listen: &LanHttpArgs) -> Result<(), ParseAction> {
    if let Some(token) = listen.token.as_deref()
        && !is_valid_web_token(token)
    {
        return Err(ParseAction::error(
            "--token must contain 16 to 128 ASCII letters, digits, '-' or '_'",
        ));
    }
    Ok(())
}

pub(super) fn http(args: Vec<String>) -> Result<HttpArgs, ParseAction> {
    let mut out = HttpArgs::default();
    let mut iter = ArgsIter::new(args);
    while let Some(arg) = iter.next() {
        if is_help(&arg) {
            return Err(ParseAction::help(HTTP_HELP));
        }
        if lan_option(&arg, &mut iter, &mut out.listen)? {
            continue;
        }
        if arg.starts_with('-') {
            return Err(ParseAction::error(format!("unknown option `{arg}`")));
        }
        if out.dir.replace(PathBuf::from(arg)).is_some() {
            return Err(ParseAction::error("http accepts only one directory"));
        }
    }
    validate_lan(&out.listen)?;
    Ok(out)
}

pub(super) fn paste(args: Vec<String>) -> Result<PasteArgs, ParseAction> {
    let mut out = PasteArgs::default();
    let mut iter = ArgsIter::new(args);
    let mut literal = false;
    while let Some(arg) = iter.next() {
        if !literal && is_help(&arg) {
            return Err(ParseAction::help(PASTE_HELP));
        }
        if !literal && arg == "--" {
            literal = true;
            continue;
        }
        if !literal && lan_option(&arg, &mut iter, &mut out.listen)? {
            continue;
        }
        if !literal {
            match split_long_value(&arg) {
                Some(("ttl", value)) => out.ttl = Some(parse_duration("--ttl", value)?),
                Some((flag, _)) => {
                    return Err(ParseAction::error(format!("unknown option `--{flag}`")));
                }
                None if arg == "--ttl" => {
                    out.ttl = Some(parse_duration("--ttl", &iter.value("--ttl")?)?)
                }
                None if arg.starts_with('-') => {
                    return Err(ParseAction::error(format!("unknown option `{arg}`")));
                }
                None => {
                    if out.text.replace(arg).is_some() {
                        return Err(ParseAction::error("paste accepts only one text value"));
                    }
                }
            }
        } else if out.text.replace(arg).is_some() {
            return Err(ParseAction::error("paste accepts only one text value"));
        }
    }
    validate_lan(&out.listen)?;
    Ok(out)
}

pub(super) fn drop(args: Vec<String>) -> Result<DropArgs, ParseAction> {
    let mut out = DropArgs::default();
    let mut iter = ArgsIter::new(args);
    while let Some(arg) = iter.next() {
        if is_help(&arg) {
            return Err(ParseAction::help(DROP_HELP));
        }
        if lan_option(&arg, &mut iter, &mut out.listen)? {
            continue;
        }
        if arg.starts_with('-') {
            return Err(ParseAction::error(format!("unknown option `{arg}`")));
        }
        if out.dir.replace(PathBuf::from(arg)).is_some() {
            return Err(ParseAction::error("drop accepts only one directory"));
        }
    }
    validate_lan(&out.listen)?;
    Ok(out)
}

pub(super) fn proxy(args: Vec<String>) -> Result<ProxyArgs, ParseAction> {
    let mut out = ProxyArgs::default();
    let mut iter = ArgsIter::new(args);
    while let Some(arg) = iter.next() {
        match split_long_value(&arg) {
            Some(("port", value)) => out.port = Some(parse_port("--port", value)?),
            Some(("bind", value)) => out.bind = Some(parse_bind("--bind", value)?),
            Some(("username", value)) => out.username = Some(value.to_string()),
            Some(("password", value)) => out.password = Some(value.to_string()),
            Some((flag, _)) => {
                return Err(ParseAction::error(format!("unknown option `--{flag}`")));
            }
            None => match arg.as_str() {
                "-h" | "--help" => return Err(ParseAction::help(PROXY_HELP)),
                "--port" => out.port = Some(parse_port("--port", &iter.value("--port")?)?),
                "--bind" => out.bind = Some(parse_bind("--bind", &iter.value("--bind")?)?),
                "--username" => out.username = Some(iter.value("--username")?),
                "--password" => out.password = Some(iter.value("--password")?),
                _ => return Err(ParseAction::error(format!("proxy does not accept `{arg}`"))),
            },
        }
    }
    if out.username.is_some() != out.password.is_some() {
        return Err(ParseAction::error(
            "--username and --password must be provided together",
        ));
    }
    Ok(out)
}

fn forward(
    args: Vec<String>,
    command: &str,
    help: &'static str,
) -> Result<ForwardArgs, ParseAction> {
    let mut target = None;
    let mut port = None;
    let mut bind = None;
    let mut iter = ArgsIter::new(args);
    while let Some(arg) = iter.next() {
        match split_long_value(&arg) {
            Some(("port", value)) => port = Some(parse_port("--port", value)?),
            Some(("bind", value)) => bind = Some(parse_bind("--bind", value)?),
            Some((flag, _)) => {
                return Err(ParseAction::error(format!("unknown option `--{flag}`")));
            }
            None => match arg.as_str() {
                "-h" | "--help" => return Err(ParseAction::help(help)),
                "--port" => port = Some(parse_port("--port", &iter.value("--port")?)?),
                "--bind" => bind = Some(parse_bind("--bind", &iter.value("--bind")?)?),
                _ if arg.starts_with('-') => {
                    return Err(ParseAction::error(format!("unknown option `{arg}`")));
                }
                _ if target.replace(parse_tunnel_target(&arg)?).is_some() => {
                    return Err(ParseAction::error(format!(
                        "{command} accepts only one target"
                    )));
                }
                _ => {}
            },
        }
    }
    let Some(target) = target else {
        return Err(ParseAction::error(format!(
            "{command} requires <host:port>"
        )));
    };
    Ok(ForwardArgs { target, port, bind })
}

pub(super) fn tcp(args: Vec<String>) -> Result<ForwardArgs, ParseAction> {
    forward(args, "tcp", TCP_HELP)
}

pub(super) fn udp(args: Vec<String>) -> Result<ForwardArgs, ParseAction> {
    forward(args, "udp", UDP_HELP)
}

pub(super) fn ping(args: Vec<String>) -> Result<PingArgs, ParseAction> {
    let mut target = None;
    let mut count = 4;
    let mut interval = Duration::from_secs(1);
    let mut timeout = Duration::from_secs(3);
    let mut iter = ArgsIter::new(args);
    while let Some(arg) = iter.next() {
        match split_long_value(&arg) {
            Some(("count", value)) => count = parse_count("--count", value)?,
            Some(("interval", value)) => interval = parse_duration("--interval", value)?,
            Some(("timeout", value)) => timeout = parse_duration("--timeout", value)?,
            Some((flag, _)) => {
                return Err(ParseAction::error(format!("unknown option `--{flag}`")));
            }
            None => match arg.as_str() {
                "-h" | "--help" => return Err(ParseAction::help(PING_HELP)),
                "--count" => count = parse_count("--count", &iter.value("--count")?)?,
                "--interval" => {
                    interval = parse_duration("--interval", &iter.value("--interval")?)?
                }
                "--timeout" => timeout = parse_duration("--timeout", &iter.value("--timeout")?)?,
                _ if arg.starts_with('-') => {
                    return Err(ParseAction::error(format!("unknown option `{arg}`")));
                }
                _ if target.replace(parse_tunnel_target(&arg)?).is_some() => {
                    return Err(ParseAction::error("ping accepts only one target"));
                }
                _ => {}
            },
        }
    }
    Ok(PingArgs {
        target: target.ok_or_else(|| ParseAction::error("ping requires <host:port>"))?,
        count,
        interval,
        timeout,
    })
}

fn parse_count(flag: &str, value: &str) -> Result<u32, ParseAction> {
    value
        .parse::<u32>()
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| ParseAction::error(format!("{flag} expects a positive integer")))
}

pub(super) fn speed(args: Vec<String>) -> Result<SpeedArgs, ParseAction> {
    let mut iter = ArgsIter::new(args);
    let Some(first) = iter.next() else {
        return Err(ParseAction::help(SPEED_HELP));
    };
    if is_help(&first) {
        return Err(ParseAction::help(SPEED_HELP));
    }
    if first == "serve" {
        let mut listen = LanHttpArgs::default();
        while let Some(arg) = iter.next() {
            if is_help(&arg) {
                return Err(ParseAction::help(SPEED_HELP));
            }
            if !lan_option(&arg, &mut iter, &mut listen)? {
                return Err(ParseAction::error(format!(
                    "speed serve does not accept `{arg}`"
                )));
            }
        }
        validate_lan(&listen)?;
        return Ok(SpeedArgs::Serve { listen });
    }
    let url = parse_http_url("speed", &first)?;
    let mut duration = Duration::from_secs(10);
    while let Some(arg) = iter.next() {
        match split_long_value(&arg) {
            Some(("duration", value)) => duration = parse_duration("--duration", value)?,
            Some((flag, _)) => {
                return Err(ParseAction::error(format!("unknown option `--{flag}`")));
            }
            None if arg == "--duration" => {
                duration = parse_duration("--duration", &iter.value("--duration")?)?
            }
            None => return Err(ParseAction::error(format!("speed does not accept `{arg}`"))),
        }
    }
    Ok(SpeedArgs::Test { url, duration })
}

pub(super) fn wake(args: Vec<String>) -> Result<WakeArgs, ParseAction> {
    let mut mac = None;
    let mut broadcast = IpAddr::V4(std::net::Ipv4Addr::BROADCAST);
    let mut port = 9;
    let mut iter = ArgsIter::new(args);
    while let Some(arg) = iter.next() {
        match split_long_value(&arg) {
            Some(("broadcast", value)) => broadcast = parse_broadcast(value)?,
            Some(("port", value)) => port = parse_port("--port", value)?,
            Some((flag, _)) => {
                return Err(ParseAction::error(format!("unknown option `--{flag}`")));
            }
            None => match arg.as_str() {
                "-h" | "--help" => return Err(ParseAction::help(WAKE_HELP)),
                "--broadcast" => broadcast = parse_broadcast(&iter.value("--broadcast")?)?,
                "--port" => port = parse_port("--port", &iter.value("--port")?)?,
                _ if arg.starts_with('-') => {
                    return Err(ParseAction::error(format!("unknown option `{arg}`")));
                }
                _ if mac.replace(parse_mac(&arg)?).is_some() => {
                    return Err(ParseAction::error("wake accepts only one MAC address"));
                }
                _ => {}
            },
        }
    }
    Ok(WakeArgs {
        mac: mac.ok_or_else(|| ParseAction::error("wake requires a MAC address"))?,
        broadcast,
        port,
    })
}

fn parse_broadcast(value: &str) -> Result<IpAddr, ParseAction> {
    value
        .parse()
        .map_err(|_| ParseAction::error("--broadcast expects an IPv4 or IPv6 address"))
}

fn parse_mac(value: &str) -> Result<[u8; 6], ParseAction> {
    let parts: Vec<_> = value.split([':', '-']).collect();
    if parts.len() != 6 || parts.iter().any(|part| part.len() != 2) {
        return Err(ParseAction::error("wake expects aa:bb:cc:dd:ee:ff"));
    }
    let mut mac = [0u8; 6];
    for (index, part) in parts.into_iter().enumerate() {
        mac[index] = u8::from_str_radix(part, 16)
            .map_err(|_| ParseAction::error("wake expects aa:bb:cc:dd:ee:ff"))?;
    }
    Ok(mac)
}

pub(super) fn port(args: Vec<String>) -> Result<PortArgs, ParseAction> {
    let mut host = None;
    let mut ports = Vec::new();
    let mut timeout = Duration::from_secs(3);
    let mut iter = ArgsIter::new(args);
    while let Some(arg) = iter.next() {
        match split_long_value(&arg) {
            Some(("timeout", value)) => timeout = parse_duration("--timeout", value)?,
            Some((flag, _)) => {
                return Err(ParseAction::error(format!("unknown option `--{flag}`")));
            }
            None if arg == "-h" || arg == "--help" => return Err(ParseAction::help(PORT_HELP)),
            None if arg == "--timeout" => {
                timeout = parse_duration("--timeout", &iter.value("--timeout")?)?
            }
            None if arg.starts_with('-') => {
                return Err(ParseAction::error(format!("unknown option `{arg}`")));
            }
            None if host.is_none() => host = Some(parse_host(&arg)?),
            None => ports.push(parse_port("port", &arg)?),
        }
    }
    Ok(PortArgs {
        host: host
            .ok_or_else(|| ParseAction::error("port requires a host and one or more ports"))?,
        ports: (!ports.is_empty())
            .then_some(ports)
            .ok_or_else(|| ParseAction::error("port requires one or more ports"))?,
        timeout,
    })
}

fn parse_host(value: &str) -> Result<String, ParseAction> {
    if value.is_empty()
        || value
            .bytes()
            .any(|byte| byte.is_ascii_whitespace() || byte == b'/')
    {
        return Err(ParseAction::error("host is invalid"));
    }
    Ok(value.to_string())
}

pub(super) fn health(args: Vec<String>) -> Result<HealthArgs, ParseAction> {
    let mut target = None;
    let mut interval = None;
    let mut timeout = Duration::from_secs(3);
    let mut iter = ArgsIter::new(args);
    while let Some(arg) = iter.next() {
        match split_long_value(&arg) {
            Some(("interval", value)) => interval = Some(parse_duration("--interval", value)?),
            Some(("timeout", value)) => timeout = parse_duration("--timeout", value)?,
            Some((flag, _)) => {
                return Err(ParseAction::error(format!("unknown option `--{flag}`")));
            }
            None => match arg.as_str() {
                "-h" | "--help" => return Err(ParseAction::help(HEALTH_HELP)),
                "--interval" => {
                    interval = Some(parse_duration("--interval", &iter.value("--interval")?)?)
                }
                "--timeout" => timeout = parse_duration("--timeout", &iter.value("--timeout")?)?,
                _ if arg.starts_with('-') => {
                    return Err(ParseAction::error(format!("unknown option `{arg}`")));
                }
                _ if target.replace(parse_health_target(&arg)?).is_some() => {
                    return Err(ParseAction::error("health accepts only one target"));
                }
                _ => {}
            },
        }
    }
    Ok(HealthArgs {
        target: target.ok_or_else(|| ParseAction::error("health requires a target"))?,
        interval,
        timeout,
    })
}

fn parse_health_target(value: &str) -> Result<String, ParseAction> {
    if value.starts_with("http://") || value.starts_with("https://") {
        let url = url::Url::parse(value)
            .map_err(|_| ParseAction::error("health expects an HTTP(S) URL or host:port"))?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || url.username() != ""
            || url.password().is_some()
        {
            return Err(ParseAction::error(
                "health expects an HTTP(S) URL or host:port",
            ));
        }
        return Ok(value.to_string());
    }
    parse_tunnel_target(value)
}

fn parse_http_url(command: &str, value: &str) -> Result<String, ParseAction> {
    let url = url::Url::parse(value)
        .map_err(|_| ParseAction::error(format!("{command} expects an http:// URL")))?;
    if url.scheme() != "http"
        || url.host_str().is_none()
        || url.username() != ""
        || url.password().is_some()
    {
        return Err(ParseAction::error(format!(
            "{command} expects an http:// URL"
        )));
    }
    Ok(value.to_string())
}

pub(super) fn pac(args: Vec<String>) -> Result<PacArgs, ParseAction> {
    let mut proxy = None;
    let mut listen = LanHttpArgs::default();
    let mut iter = ArgsIter::new(args);
    while let Some(arg) = iter.next() {
        if is_help(&arg) {
            return Err(ParseAction::help(PAC_HELP));
        }
        if lan_option(&arg, &mut iter, &mut listen)? {
            continue;
        }
        match split_long_value(&arg) {
            Some(("proxy", value)) => proxy = Some(parse_proxy_url(value)?),
            Some((flag, _)) => {
                return Err(ParseAction::error(format!("unknown option `--{flag}`")));
            }
            None if arg == "--proxy" => proxy = Some(parse_proxy_url(&iter.value("--proxy")?)?),
            None => return Err(ParseAction::error(format!("pac does not accept `{arg}`"))),
        }
    }
    validate_lan(&listen)?;
    Ok(PacArgs {
        proxy: proxy.ok_or_else(|| ParseAction::error("pac requires --proxy <url>"))?,
        listen,
    })
}

fn parse_proxy_url(value: &str) -> Result<String, ParseAction> {
    let url = url::Url::parse(value).map_err(|_| {
        ParseAction::error("--proxy expects http://host:port or socks5://host:port")
    })?;
    if !matches!(url.scheme(), "http" | "socks5")
        || url.host_str().is_none()
        || url.port().is_none()
        || url.username() != ""
        || url.password().is_some()
        || !matches!(url.path(), "" | "/")
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(ParseAction::error(
            "--proxy expects http://host:port or socks5://host:port",
        ));
    }
    Ok(value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lan_commands_accept_token_and_equals_options() {
        let args = http(vec![
            "shared".to_string(),
            "--port=4567".to_string(),
            "--token=A1b2C3d4E5f6G7h8".to_string(),
        ])
        .unwrap();
        assert_eq!(args.dir, Some(PathBuf::from("shared")));
        assert_eq!(args.listen.port, Some(4567));

        let args = paste(vec!["--ttl=2s".to_string(), "hello".to_string()]).unwrap();
        assert_eq!(args.text.as_deref(), Some("hello"));
        assert_eq!(args.ttl, Some(Duration::from_secs(2)));
    }

    #[test]
    fn drop_and_pac_parse_their_own_options() {
        let drop_args = drop(vec!["uploads".to_string(), "--bind=::1".to_string()]).unwrap();
        assert_eq!(drop_args.dir, Some(PathBuf::from("uploads")));
        assert_eq!(drop_args.listen.bind, Some("::1".parse().unwrap()));

        let pac_args = pac(vec!["--proxy=socks5://127.0.0.1:1080".to_string()]).unwrap();
        assert_eq!(pac_args.proxy, "socks5://127.0.0.1:1080");
    }

    #[test]
    fn network_commands_reject_invalid_inputs() {
        assert!(proxy(vec!["--username=user".to_string()]).is_err());
        assert!(wake(vec!["invalid".to_string()]).is_err());
        assert!(speed(vec!["https://example.test".to_string()]).is_err());
        assert!(port(vec!["localhost".to_string()]).is_err());
        assert!(ping(vec!["localhost:80".to_string(), "--count=0".to_string()]).is_err());
    }

    #[test]
    fn health_accepts_https_and_bare_token_generates_a_valid_token() {
        assert_eq!(
            health(vec!["https://health.example.test/status".to_string()])
                .unwrap()
                .target,
            "https://health.example.test/status"
        );
        let args = paste(vec!["--token".to_string()]).unwrap();
        assert!(args.listen.token.as_deref().is_some_and(is_valid_web_token));
    }
}
