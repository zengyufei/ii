use super::*;

pub(super) fn parse(args: Vec<String>) -> Result<DavArgs, ParseAction> {
    let mut out = DavArgs::default();
    let mut iter = ArgsIter::new(args);
    while let Some(arg) = iter.next() {
        match split_long_value(&arg) {
            Some(("port", value)) => out.web_port = Some(parse_port("--port", value)?),
            Some(("bind", value)) => out.web_bind = Some(parse_bind("--bind", value)?),
            Some(("token", value)) => out.web_token = Some(value.to_string()),
            Some(("username", value)) => out.username = Some(value.to_string()),
            Some(("password", value)) => out.password = Some(value.to_string()),
            Some(("domain", value)) => out.domain = Some(parse_tls_domain(value)?),
            Some(("cert", value)) => out.cert = Some(PathBuf::from(value)),
            Some(("key", value)) => out.key = Some(PathBuf::from(value)),
            Some(("read-only", _)) => {
                return Err(ParseAction::error("--read-only does not take a value"));
            }
            Some(("tls", _)) => return Err(ParseAction::error("--tls does not take a value")),
            Some((flag, _)) => {
                return Err(ParseAction::error(format!("unknown option `--{flag}`")));
            }
            None => match arg.as_str() {
                "-h" | "--help" => return Err(ParseAction::help(DAV_HELP)),
                "--port" => out.web_port = Some(parse_port("--port", &iter.value("--port")?)?),
                "--bind" => out.web_bind = Some(parse_bind("--bind", &iter.value("--bind")?)?),
                "--token" => out.web_token = Some(web_token(&mut iter)),
                "--read-only" => out.read_only = true,
                "--username" => out.username = Some(iter.value("--username")?),
                "--password" => out.password = Some(iter.value("--password")?),
                "--tls" => out.tls = true,
                "--domain" => out.domain = Some(parse_tls_domain(&iter.value("--domain")?)?),
                "--cert" => out.cert = Some(PathBuf::from(iter.value("--cert")?)),
                "--key" => out.key = Some(PathBuf::from(iter.value("--key")?)),
                _ if arg.starts_with('-') => {
                    return Err(ParseAction::error(format!("unknown option `{arg}`")));
                }
                _ if out.dir.replace(PathBuf::from(&arg)).is_some() => {
                    return Err(ParseAction::error("dav accepts only one directory"));
                }
                _ => {}
            },
        }
    }
    if let Some(token) = out.web_token.as_deref()
        && !is_valid_web_token(token)
    {
        return Err(ParseAction::error(
            "--token must contain 16 to 128 ASCII letters, digits, '-' or '_'",
        ));
    }
    if out.username.is_some() != out.password.is_some() {
        return Err(ParseAction::error(
            "--username and --password must be provided together",
        ));
    }
    if let Some(username) = out.username.as_deref()
        && (username.is_empty() || username.contains([':', '\r', '\n']))
    {
        return Err(ParseAction::error(
            "--username must not be empty or contain ':', CR, or LF",
        ));
    }
    if let Some(password) = out.password.as_deref()
        && (password.is_empty() || password.contains(['\r', '\n']))
    {
        return Err(ParseAction::error(
            "--password must not be empty or contain CR or LF",
        ));
    }
    if out.domain.is_some() && !out.tls {
        return Err(ParseAction::error("--domain requires --tls"));
    }
    if (out.cert.is_some() || out.key.is_some()) && !out.tls {
        return Err(ParseAction::error("--cert and --key require --tls"));
    }
    if out.cert.is_some() != out.key.is_some() {
        return Err(ParseAction::error(
            "--cert and --key must be provided together",
        ));
    }
    Ok(out)
}
