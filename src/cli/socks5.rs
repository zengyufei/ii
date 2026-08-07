use super::*;

pub(super) fn parse(args: Vec<String>) -> Result<Socks5Args, ParseAction> {
    let mut out = Socks5Args::default();
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
                "-h" | "--help" => return Err(ParseAction::help(SOCKS5_HELP)),
                "--port" => out.port = Some(parse_port("--port", &iter.value("--port")?)?),
                "--bind" => out.bind = Some(parse_bind("--bind", &iter.value("--bind")?)?),
                "--username" => out.username = Some(iter.value("--username")?),
                "--password" => out.password = Some(iter.value("--password")?),
                _ if arg.starts_with('-') => {
                    return Err(ParseAction::error(format!("unknown option `{arg}`")));
                }
                _ => {
                    return Err(ParseAction::error(
                        "socks5 does not accept positional arguments",
                    ));
                }
            },
        }
    }
    if out.username.is_some() != out.password.is_some() {
        return Err(ParseAction::error(
            "--username and --password must be provided together",
        ));
    }
    if let Some(username) = out.username.as_deref()
        && (username.is_empty() || username.as_bytes().len() > u8::MAX as usize)
    {
        return Err(ParseAction::error("--username must contain 1 to 255 bytes"));
    }
    if let Some(password) = out.password.as_deref()
        && password.as_bytes().len() > u8::MAX as usize
    {
        return Err(ParseAction::error(
            "--password must contain at most 255 bytes",
        ));
    }
    Ok(out)
}
