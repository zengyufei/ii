use super::*;

pub(super) fn parse(args: Vec<String>) -> Result<TunnelArgs, ParseAction> {
    let mut serve_target = None;
    let mut connect_ticket = None;
    let mut listen = None;
    let mut relay = None;
    let mut accept_self_signed_relay = false;
    let mut iter = ArgsIter::new(args);

    while let Some(arg) = iter.next() {
        match split_long_value(&arg) {
            Some(("listen", value)) => listen = Some(parse_listen_addr("--listen", value)?),
            Some(("relay", value)) => relay = Some(parse_relay_url(value)?),
            Some((flag, _)) => {
                return Err(ParseAction::error(format!("unknown option `--{flag}`")));
            }
            None => match arg.as_str() {
                "-h" | "--help" => return Err(ParseAction::help(TUNNEL_HELP)),
                "-s" => {
                    if serve_target
                        .replace(parse_tunnel_target(&iter.value("-s")?)?)
                        .is_some()
                    {
                        return Err(ParseAction::error("tunnel accepts only one -s target"));
                    }
                }
                "-c" => {
                    if connect_ticket.replace(iter.value("-c")?).is_some() {
                        return Err(ParseAction::error("tunnel accepts only one -c ticket"));
                    }
                }
                "--listen" => {
                    listen = Some(parse_listen_addr("--listen", &iter.value("--listen")?)?)
                }
                "--relay" => relay = Some(parse_relay_url(&iter.value("--relay")?)?),
                "-k" => accept_self_signed_relay = true,
                _ if arg.starts_with('-') => {
                    return Err(ParseAction::error(format!("unknown option `{arg}`")));
                }
                _ => return Err(ParseAction::error(format!("unexpected argument `{arg}`"))),
            },
        }
    }

    match (serve_target, connect_ticket) {
        (Some(_), Some(_)) => Err(ParseAction::error("-s conflicts with -c")),
        (Some(target), None) => {
            if listen.is_some() {
                return Err(ParseAction::error("--listen requires -c <ticket>"));
            }
            if accept_self_signed_relay && relay.is_none() {
                return Err(ParseAction::error("-k requires -s --relay <url>"));
            }
            if accept_self_signed_relay && relay.as_ref().is_some_and(|url| url.scheme() != "https")
            {
                return Err(ParseAction::error("-k requires an https:// relay URL"));
            }
            Ok(TunnelArgs::Serve {
                target,
                relay,
                accept_self_signed_relay,
            })
        }
        (None, Some(ticket)) => {
            if relay.is_some() {
                return Err(ParseAction::error("--relay requires -s <target-host:port>"));
            }
            if accept_self_signed_relay {
                return Err(ParseAction::error("-k requires -s --relay <url>"));
            }
            Ok(TunnelArgs::Connect { ticket, listen })
        }
        (None, None) => Err(ParseAction::error(
            "tunnel requires -s <target-host:port> or -c <ticket>",
        )),
    }
}
