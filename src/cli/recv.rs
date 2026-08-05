use super::*;

pub(super) fn parse(args: Vec<String>) -> Result<RecvArgs, ParseAction> {
    let mut ticket = None;
    let mut out_dir = None;
    let mut stdout = false;
    let mut overwrite = false;
    let mut resume = false;
    let mut local = false;
    let mut trace = false;
    let mut json = false;
    let mut iter = ArgsIter::new(args);

    while let Some(arg) = iter.next() {
        match split_long_value(&arg) {
            Some(("output", value)) => out_dir = Some(PathBuf::from(value)),
            Some((flag, _)) => {
                return Err(ParseAction::error(format!("unknown option `--{flag}`")));
            }
            None => match arg.as_str() {
                "-h" | "--help" => return Err(ParseAction::help(RECV_HELP)),
                "-o" => out_dir = Some(PathBuf::from(iter.value("-o")?)),
                "--stdout" => stdout = true,
                "--overwrite" => overwrite = true,
                "--resume" => resume = true,
                "--local" => local = true,
                "--trace" => trace = true,
                "--json" => json = true,
                _ if arg.starts_with('-') => {
                    return Err(ParseAction::error(format!("unknown option `{arg}`")));
                }
                _ => {
                    if ticket.replace(arg).is_some() {
                        return Err(ParseAction::error("recv accepts only one ticket"));
                    }
                }
            },
        }
    }

    if stdout && (resume || json) {
        return Err(ParseAction::error(if json {
            "--stdout conflicts with --json"
        } else {
            "--stdout conflicts with --resume"
        }));
    }

    let Some(ticket) = ticket else {
        return Err(ParseAction::error("missing ticket"));
    };

    Ok(RecvArgs {
        ticket,
        out_dir,
        stdout,
        overwrite,
        resume,
        local,
        trace,
        json,
    })
}
