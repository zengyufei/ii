use super::*;

pub(super) fn parse(args: Vec<String>) -> Result<RelayArgs, ParseAction> {
    let mut public = None;
    let mut tls_domain = None;
    let mut cert = None;
    let mut key = None;
    let mut port = None;
    let mut iter = ArgsIter::new(args);

    while let Some(arg) = iter.next() {
        match split_long_value(&arg) {
            Some(("public", value)) => public = Some(parse_public_relay_url(value)?),
            Some(("tls", value)) => tls_domain = Some(parse_tls_domain(value)?),
            Some(("cert", value)) => cert = Some(PathBuf::from(value)),
            Some(("key", value)) => key = Some(PathBuf::from(value)),
            Some((flag, _)) => {
                return Err(ParseAction::error(format!("unknown option `--{flag}`")));
            }
            None => match arg.as_str() {
                "-h" | "--help" => return Err(ParseAction::help(RELAY_HELP)),
                "--public" => public = Some(parse_public_relay_url(&iter.value("--public")?)?),
                "--tls" => tls_domain = Some(parse_tls_domain(&iter.value("--tls")?)?),
                "--cert" => cert = Some(PathBuf::from(iter.value("--cert")?)),
                "--key" => key = Some(PathBuf::from(iter.value("--key")?)),
                "-H" => port = Some(parse_port("-H", &iter.value("-H")?)?),
                _ => return Err(ParseAction::error(format!("unexpected argument `{arg}`"))),
            },
        }
    }

    match (&public, &tls_domain, &cert, &key) {
        (Some(_), None, None, None) => {}
        (Some(_), _, _, _) => {
            return Err(ParseAction::error(
                "--public conflicts with --tls, --cert, and --key",
            ));
        }
        (None, Some(_), Some(_), Some(_)) => {}
        (None, Some(_), _, _) => {
            return Err(ParseAction::error(
                "--tls requires both --cert <path> and --key <path>",
            ));
        }
        (None, None, Some(_), _) | (None, None, _, Some(_)) => {
            return Err(ParseAction::error(
                "--cert and --key require --tls <domain>",
            ));
        }
        (None, None, None, None) => {
            return Err(ParseAction::error(
                "ii relay requires --public <https-url> or --tls <domain> --cert <path> --key <path>",
            ));
        }
    }

    Ok(RelayArgs {
        public,
        tls_domain,
        cert,
        key,
        port,
    })
}
