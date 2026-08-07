use crate::{
    command::{SendArgs, WebArgs},
    ticket::PayloadKind,
    transport::{progress::RateLimiter, source::Source},
    web::{WebContent, WebServeLifetime, directory_root, serve_web, web_upload_dir},
};
use anyhow::{Context, Result};
use std::sync::Arc;

pub(super) async fn run(args: WebArgs) -> Result<()> {
    run_impl(args).await
}

pub(super) async fn send_web(args: SendArgs) -> Result<()> {
    if args.preserve_metadata {
        anyhow::bail!("--preserve-metadata cannot be used with --web");
    }
    let source =
        Source::open_with_options(args.path.clone(), args.name.clone(), args.symlinks, false)
            .await?;
    if let Some(algorithm) = args.checksum {
        let value = source.checksum(algorithm).await?;
        if args.json {
            crate::json::emit(
                "checksum",
                &[
                    ("operation", crate::json::Value::String("send")),
                    ("algorithm", crate::json::Value::String(algorithm.name())),
                    ("value", crate::json::Value::String(&value)),
                ],
            );
        } else {
            println!("checksum ({}): {}", algorithm.name(), value);
        }
    }
    let download_name = match source.kind() {
        PayloadKind::Dir => format!("{}.tar", source.name()),
        PayloadKind::File | PayloadKind::Stdin => source.name().to_string(),
    };
    let upload_dir = args
        .web_upload
        .then(|| std::env::current_dir().context("read current directory for web uploads"))
        .transpose()?
        .map(|start_dir| web_upload_dir(&start_dir, args.web_upload_dir.as_deref()));
    serve_web(
        WebContent::Download {
            source,
            download_name,
            download_qr_svg: String::new(),
        },
        upload_dir,
        args.web_port,
        args.web_bind,
        args.web_token,
        args.rate.map(RateLimiter::new).map(Arc::new),
        args.json,
        if args.keep_alive {
            WebServeLifetime::UntilCtrlC
        } else {
            WebServeLifetime::OneSuccessfulDownload
        },
    )
    .await
}
async fn run_impl(args: WebArgs) -> Result<()> {
    let start_dir = std::env::current_dir().context("read current directory for web service")?;
    let root = directory_root(&start_dir, args.dir.as_deref()).await?;
    let upload_dir = args
        .web_upload
        .then(|| web_upload_dir(&start_dir, args.web_upload_dir.as_deref()));
    serve_web(
        WebContent::Directory { root },
        upload_dir,
        args.web_port,
        args.web_bind,
        args.web_token,
        None,
        false,
        if args.once {
            WebServeLifetime::OneSuccessfulDownload
        } else {
            WebServeLifetime::UntilCtrlC
        },
    )
    .await
}
