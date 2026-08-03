use super::*;

pub(super) fn parse(args: Vec<String>) -> Result<RelayArgs, ParseAction> {
    let mut tls = false;
    let mut domain = None;
    let mut cert = None;
    let mut key = None;
    let mut port = None;
    let mut iter = ArgsIter::new(args);

    while let Some(arg) = iter.next() {
        match split_long_value(&arg) {
            Some(("port", value)) => port = Some(parse_port("--port", value)?),
            Some(("domain", value)) => domain = Some(parse_tls_domain(value)?),
            Some(("cert", value)) => cert = Some(PathBuf::from(value)),
            Some(("key", value)) => key = Some(PathBuf::from(value)),
            Some(("tls", _)) => {
                return Err(ParseAction::error("--tls does not take a value"));
            }
            Some((flag, _)) => {
                return Err(ParseAction::error(format!("unknown option `--{flag}`")));
            }
            None => match arg.as_str() {
                "-h" | "--help" => return Err(ParseAction::help(RELAY_HELP)),
                "--port" => port = Some(parse_port("--port", &iter.value("--port")?)?),
                "--tls" => tls = true,
                "--domain" => domain = Some(parse_tls_domain(&iter.value("--domain")?)?),
                "--cert" => cert = Some(PathBuf::from(iter.value("--cert")?)),
                "--key" => key = Some(PathBuf::from(iter.value("--key")?)),
                _ => return Err(ParseAction::error(format!("unexpected argument `{arg}`"))),
            },
        }
    }

    if domain.is_some() && !tls {
        return Err(ParseAction::error("--domain requires --tls"));
    }
    if (cert.is_some() || key.is_some()) && !tls {
        return Err(ParseAction::error("--cert and --key require --tls"));
    }
    if cert.is_some() != key.is_some() {
        return Err(ParseAction::error(
            "--cert and --key must be provided together",
        ));
    }

    Ok(RelayArgs {
        tls,
        domain,
        cert,
        key,
        port,
    })
}
