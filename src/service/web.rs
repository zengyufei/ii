use crate::{
    command::{SendArgs, WebArgs},
    ticket::PayloadKind,
    transport::source::Source,
    web::{WebContent, directory_root, serve_web, web_upload_dir},
};
use anyhow::{Context, Result};

pub(super) async fn run(args: WebArgs) -> Result<()> {
    run_impl(args).await
}

pub(super) async fn send_web(args: SendArgs) -> Result<()> {
    let source = Source::open(args.path.clone(), args.name.clone()).await?;
    let download_name = match source.kind() {
        PayloadKind::Dir => format!("{}.tar", source.name()),
        PayloadKind::File | PayloadKind::Stdin => source.name().to_string(),
    };
    let start_dir = std::env::current_dir().context("read current directory for web uploads")?;
    let upload_dir = web_upload_dir(&start_dir, args.web_upload_dir.as_deref());
    serve_web(
        WebContent::Download {
            source,
            download_name,
            download_qr_svg: String::new(),
        },
        upload_dir,
        args.web_port,
        args.web_token,
    )
    .await
}
async fn run_impl(args: WebArgs) -> Result<()> {
    let start_dir = std::env::current_dir().context("read current directory for web service")?;
    let root = directory_root(&start_dir, args.dir.as_deref()).await?;
    let upload_dir = web_upload_dir(&start_dir, args.web_upload_dir.as_deref());
    serve_web(
        WebContent::Directory { root },
        upload_dir,
        args.web_port,
        args.web_token,
    )
    .await
}
