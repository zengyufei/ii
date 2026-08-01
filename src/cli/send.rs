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
            Some(("token", value)) => out.web_token = Some(value.to_string()),
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
                "--ftp" => out.ftp = true,
                "-d" => out.delete_after_recv = true,
                "--profile" => out.profile = Some(iter.value("--profile")?),
                "--webdav" => out.webdav = true,
                "--sftp" => out.sftp = true,
                "--web" => out.web = true,
                "--token" => out.web_token = Some(iter.value("--token")?),
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
                    if out.path.replace(PathBuf::from(&arg)).is_some() {
                        return Err(ParseAction::error("send accepts only one path"));
                    }
                }
            },
        }
    }

    validate_send(&out)?;
    Ok(out)
}
