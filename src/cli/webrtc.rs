use super::*;

pub(super) fn parse(args: Vec<String>) -> Result<WebrtcArgs, ParseAction> {
    let mut out = WebrtcArgs::default();
    let mut iter = ArgsIter::new(args);

    while let Some(arg) = iter.next() {
        match split_long_value(&arg) {
            Some(("port", value)) => out.web_port = Some(parse_port("--port", value)?),
            Some(("token", value)) => out.web_token = Some(value.to_string()),
            Some((flag, _)) => {
                return Err(ParseAction::error(format!("unknown option `--{flag}`")));
            }
            None => match arg.as_str() {
                "-h" | "--help" => return Err(ParseAction::help(WEBRTC_HELP)),
                "--port" => out.web_port = Some(parse_port("--port", &iter.value("--port")?)?),
                "--token" => out.web_token = Some(web_token(&mut iter)),
                _ if arg.starts_with('-') => {
                    return Err(ParseAction::error(format!("unknown option `{arg}`")));
                }
                _ => {
                    return Err(ParseAction::error(
                        "webrtc does not accept positional arguments",
                    ));
                }
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
