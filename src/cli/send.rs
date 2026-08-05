use super::*;

pub(super) fn parse(args: Vec<String>) -> Result<SendArgs, ParseAction> {
    let mut out = SendArgs::default();
    let mut iter = ArgsIter::new(args);

    while let Some(arg) = iter.next() {
        match split_long_value(&arg) {
            Some(("name", value)) => out.name = Some(value.to_string()),
            Some(("output", value)) => out.output = Some(PathBuf::from(value)),
            Some(("profile", value)) => out.profile = Some(value.to_string()),
            Some(("relay", value)) => out.relay = Some(parse_relay_url(value)?),
            Some(("port", value)) => out.web_port = Some(parse_port("--port", value)?),
            Some(("bind", value)) => out.web_bind = Some(parse_bind("--bind", value)?),
            Some(("token", value)) => out.web_token = Some(value.to_string()),
            Some(("include", value)) => out.include.push(validate_glob("--include", value)?),
            Some(("exclude", value)) => out.exclude.push(validate_glob("--exclude", value)?),
            Some(("rate", value)) => out.rate = Some(parse_rate("--rate", value)?),
            Some(("json", _)) => return Err(ParseAction::error("--json does not take a value")),
            Some(("upload", _)) => {
                return Err(ParseAction::error("--upload does not take a value"));
            }
            Some(("path", value)) => out.web_upload_dir = Some(PathBuf::from(value)),
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
                "--r2" => out.r2 = true,
                "--azure" => out.azure = true,
                "--ftp" => out.ftp = true,
                "-d" => out.delete_after_recv = true,
                "--profile" => out.profile = Some(iter.value("--profile")?),
                "--webdav" => out.webdav = true,
                "--sftp" => out.sftp = true,
                "--web" => out.web = true,
                "--port" => out.web_port = Some(parse_port("--port", &iter.value("--port")?)?),
                "--bind" => out.web_bind = Some(parse_bind("--bind", &iter.value("--bind")?)?),
                "--token" => out.web_token = Some(web_token(&mut iter)),
                "--include" => out
                    .include
                    .push(validate_glob("--include", &iter.value("--include")?)?),
                "--exclude" => out
                    .exclude
                    .push(validate_glob("--exclude", &iter.value("--exclude")?)?),
                "--rate" => out.rate = Some(parse_rate("--rate", &iter.value("--rate")?)?),
                "--json" => out.json = true,
                "--upload" if out.web_upload => {
                    return Err(ParseAction::error("--upload may be specified only once"));
                }
                "--upload" => out.web_upload = true,
                "--path" => out.web_upload_dir = Some(PathBuf::from(iter.value("--path")?)),
                "-p" => out.portable_webdav = true,
                "--local" => out.local = true,
                "--relay" => out.relay = Some(parse_relay_url(&iter.value("--relay")?)?),
                "-k" => out.accept_self_signed_relay = true,
                "--no-relay" => out.no_relay = true,
                _ if arg.starts_with('-') => {
                    return Err(ParseAction::error(format!("unknown option `{arg}`")));
                }
                _ => {
                    let path = PathBuf::from(&arg);
                    if out.path.is_none() {
                        out.path = Some(path);
                    } else {
                        out.extra_paths.push(path);
                    }
                }
            },
        }
    }

    validate_send(&out)?;
    Ok(out)
}
