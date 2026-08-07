use super::{
    ArgsIter, ParseAction,
    help::{QUEUE_HELP, WATCH_HELP},
    parse_duration, send,
};
use crate::command::{QueueArgs, SendArgs, WatchArgs};
use std::time::Duration;

fn validate_job_send(command: &str, args: &SendArgs) -> Result<(), ParseAction> {
    if args.keep_alive || args.copy || args.output.is_some() || args.web || args.json {
        return Err(ParseAction::error(format!(
            "{command} does not accept -t, -c, -o, --web, or --json"
        )));
    }
    Ok(())
}

pub(super) fn parse_watch(args: Vec<String>) -> Result<WatchArgs, ParseAction> {
    let mut interval = Duration::from_secs(2);
    let mut stabilize = Duration::from_secs(2);
    let mut send_args = Vec::new();
    let mut iter = ArgsIter::new(args);
    while let Some(arg) = iter.next() {
        if matches!(arg.as_str(), "-h" | "--help") {
            return Err(ParseAction::help(WATCH_HELP));
        }
        match super::common::split_long_value(&arg) {
            Some(("interval", value)) => interval = parse_duration("--interval", value)?,
            Some(("stabilize", value)) => stabilize = parse_duration("--stabilize", value)?,
            _ => match arg.as_str() {
                "--interval" => {
                    interval = parse_duration("--interval", &iter.value("--interval")?)?
                }
                "--stabilize" => {
                    stabilize = parse_duration("--stabilize", &iter.value("--stabilize")?)?
                }
                _ => send_args.push(arg),
            },
        }
    }
    let send = send::parse(send_args)?;
    let Some(dir) = send.path.clone() else {
        return Err(ParseAction::error("watch requires a directory"));
    };
    if !send.extra_paths.is_empty() {
        return Err(ParseAction::error("watch accepts only one directory"));
    }
    validate_job_send("watch", &send)?;
    Ok(WatchArgs {
        dir,
        interval,
        stabilize,
        send,
    })
}

pub(super) fn parse_queue(args: Vec<String>) -> Result<QueueArgs, ParseAction> {
    let mut after = None;
    let mut every = None;
    let mut send_args = Vec::new();
    let mut iter = ArgsIter::new(args);
    while let Some(arg) = iter.next() {
        if matches!(arg.as_str(), "-h" | "--help") {
            return Err(ParseAction::help(QUEUE_HELP));
        }
        match super::common::split_long_value(&arg) {
            Some(("after", value)) => after = Some(parse_duration("--after", value)?),
            Some(("every", value)) => every = Some(parse_duration("--every", value)?),
            _ => match arg.as_str() {
                "--after" => after = Some(parse_duration("--after", &iter.value("--after")?)?),
                "--every" => every = Some(parse_duration("--every", &iter.value("--every")?)?),
                _ => send_args.push(arg),
            },
        }
    }
    if after.is_some() && every.is_some() {
        return Err(ParseAction::error("--after conflicts with --every"));
    }
    let preserve_metadata = send_args.iter().any(|arg| arg == "--preserve-metadata");
    if preserve_metadata {
        send_args.retain(|arg| arg != "--preserve-metadata");
    }
    let mut send = send::parse(send_args)?;
    if preserve_metadata {
        send.preserve_metadata = true;
    }
    validate_job_send("queue", &send)?;
    let mut paths = Vec::new();
    if let Some(path) = send.path.clone() {
        paths.push(path);
    }
    paths.extend(send.extra_paths.iter().cloned());
    if paths.is_empty() {
        return Err(ParseAction::error("queue requires at least one path"));
    }
    send.extra_paths.clear();
    Ok(QueueArgs {
        paths,
        after,
        every,
        send,
    })
}
