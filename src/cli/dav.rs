use super::*;

pub(super) fn parse(args: Vec<String>) -> Result<DavArgs, ParseAction> {
    let mut out = DavArgs::default();
    let mut iter = ArgsIter::new(args);
    while let Some(arg) = iter.next() {
        match split_long_value(&arg) {
            Some(("port", value)) => out.web_port = Some(parse_port("--port", value)?),
            Some(("bind", value)) => out.web_bind = Some(parse_bind("--bind", value)?),
            Some(("token", value)) => out.web_token = Some(value.to_string()),
            Some(("read-only", _)) => {
                return Err(ParseAction::error("--read-only does not take a value"));
            }
            Some((flag, _)) => {
                return Err(ParseAction::error(format!("unknown option `--{flag}`")));
            }
            None => match arg.as_str() {
                "-h" | "--help" => return Err(ParseAction::help(DAV_HELP)),
                "--port" => out.web_port = Some(parse_port("--port", &iter.value("--port")?)?),
                "--bind" => out.web_bind = Some(parse_bind("--bind", &iter.value("--bind")?)?),
                "--token" => out.web_token = Some(web_token(&mut iter)),
                "--read-only" => out.read_only = true,
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
    Ok(out)
}
