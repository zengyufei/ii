use super::*;

pub(super) fn parse(args: Vec<String>) -> Result<DiscoverArgs, ParseAction> {
    let mut out = DiscoverArgs::default();
    for arg in args {
        match arg.as_str() {
            "-h" | "--help" => return Err(ParseAction::help(DISCOVER_HELP)),
            "--json" => out.json = true,
            _ if arg.starts_with('-') => {
                return Err(ParseAction::error(format!("unknown option `{arg}`")));
            }
            _ => {
                return Err(ParseAction::error(
                    "discover does not accept positional arguments",
                ));
            }
        }
    }
    Ok(out)
}
