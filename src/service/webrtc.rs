use crate::{
    command::WebrtcArgs,
    web::{WebRtcServer, serve_webrtc_connection, start_lan_web_server},
};
use anyhow::{Context, Result};
use std::sync::Arc;

pub(super) async fn run(args: WebrtcArgs) -> Result<()> {
    run_impl(args).await
}

async fn run_impl(args: WebrtcArgs) -> Result<()> {
    let server = Arc::new(WebRtcServer::new());
    let lan = start_lan_web_server(args.web_port, args.web_token.as_deref(), "ii webrtc").await?;

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            accepted = lan.listener.accept() => {
                let (stream, _) = accepted.context("accept WebRTC connection")?;
                let server = Arc::clone(&server);
                let web_token = args.web_token.clone();
                tokio::spawn(async move {
                    if let Err(err) = serve_webrtc_connection(stream, server, web_token).await {
                        eprintln!("ii webrtc: request failed: {err:#}");
                    }
                });
            }
        }
    }
    Ok(())
}
