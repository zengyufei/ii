use crate::{
    command::DavArgs,
    web::{directory_root, serve_dav},
};
use anyhow::{Context, Result};

pub(super) async fn run(args: DavArgs) -> Result<()> {
    let start_dir = std::env::current_dir().context("read current directory for DAV service")?;
    let root = directory_root(&start_dir, args.dir.as_deref()).await?;
    serve_dav(
        root,
        args.web_port,
        args.web_bind,
        args.web_token,
        args.read_only,
    )
    .await
}
