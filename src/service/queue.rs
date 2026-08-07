use crate::command::QueueArgs;
use anyhow::Result;
use tokio::time::{Duration, sleep};

pub(super) async fn run(args: QueueArgs) -> Result<()> {
    if let Some(delay) = args.after {
        if wait_or_stop(delay).await {
            return Ok(());
        }
    }
    loop {
        for path in &args.paths {
            let mut send = args.send.clone();
            send.path = Some(path.clone());
            send.extra_paths.clear();
            tokio::select! {
                result = crate::service::send(send) => {
                    if let Err(err) = result {
                        eprintln!("ii queue: {} failed: {err:#}", path.display());
                    }
                }
                _ = tokio::signal::ctrl_c() => return Ok(()),
            }
        }
        let Some(delay) = args.every else {
            return Ok(());
        };
        if wait_or_stop(delay).await {
            return Ok(());
        }
    }
}

async fn wait_or_stop(delay: Duration) -> bool {
    tokio::select! {
        _ = sleep(delay) => false,
        _ = tokio::signal::ctrl_c() => true,
    }
}
