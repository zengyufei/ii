use super::*;

pub(super) fn parse(args: Vec<String>) -> Result<WebArgs, ParseAction> {
    let mut out = WebArgs::default();
    let mut iter = ArgsIter::new(args);

    while let Some(arg) = iter.next() {
        match split_long_value(&arg) {
            Some(("token", value)) => out.web_token = Some(value.to_string()),
            Some(("path", value)) => out.web_upload_dir = Some(PathBuf::from(value)),
            Some((flag, _)) => {
                return Err(ParseAction::error(format!("unknown option `--{flag}`")));
            }
            None => match arg.as_str() {
                "-h" | "--help" => return Err(ParseAction::help(WEB_HELP)),
                "--token" => out.web_token = Some(iter.value("--token")?),
                "--path" => out.web_upload_dir = Some(PathBuf::from(iter.value("--path")?)),
                _ if arg.starts_with('-') => {
                    return Err(ParseAction::error(format!("unknown option `{arg}`")));
                }
                _ => {
                    if out.dir.replace(PathBuf::from(&arg)).is_some() {
                        return Err(ParseAction::error("web accepts only one directory"));
                    }
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
