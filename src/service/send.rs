use crate::{
    command::SendArgs,
    service::{TransferEvent, web::send_web},
    storage,
    ticket::{FtpPortableCredentials, Ticket, WebDavPortableCredentials},
    transport::{
        iroh::{FILE_ALPN, bind_endpoint, endpoint_policy_for_send, should_wait_online},
        p2p::{ServeOutcome, serve_one},
        progress::should_show_progress,
        source::Source,
    },
};
use anyhow::{Context, Result, bail};
use iroh::TransportAddr;
use std::{
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};

pub(super) async fn run(args: SendArgs) -> Result<()> {
    run_impl(args).await
}

pub(super) async fn with_events(
    args: SendArgs,
    events: std::sync::mpsc::Sender<TransferEvent>,
) -> Result<()> {
    send_with_events_impl(args, events).await
}

async fn run_impl(args: SendArgs) -> Result<()> {
    if args.web {
        return send_web(args).await;
    }
    let copy = args.copy;
    let output = args.output.clone();
    send_inner(args, move |ticket| {
        print_ticket(ticket, copy, output.clone())
    })
    .await
}
async fn send_with_events_impl(
    args: SendArgs,
    events: std::sync::mpsc::Sender<TransferEvent>,
) -> Result<()> {
    let _ = events.send(TransferEvent::Started);
    let ticket_events = events.clone();
    let result = send_inner(args, move |ticket| {
        let _ = ticket_events.send(TransferEvent::TicketReady(ticket.to_string()));
        Ok(())
    })
    .await;
    match &result {
        Ok(()) => {
            let _ = events.send(TransferEvent::Completed);
        }
        Err(err) => {
            let _ = events.send(TransferEvent::Failed(format!("{err:#}")));
        }
    }
    result
}

async fn send_inner<F>(args: SendArgs, ticket_ready: F) -> Result<()>
where
    F: Fn(&str) -> Result<()> + Send + Sync,
{
    let show_progress = should_show_progress(false);
    if args.delete_after_recv && !args.s3 && !args.webdav && !args.ftp && !args.sftp {
        bail!("-d requires --s3, --webdav, --ftp or --sftp");
    }
    if args.profile.is_some() && !args.s3 && !args.webdav && !args.ftp && !args.sftp {
        bail!("--profile requires --s3, --webdav, --ftp or --sftp");
    }
    if args.s3 {
        return send_s3(args, show_progress, &ticket_ready).await;
    }
    if args.webdav {
        return send_webdav(args, show_progress, &ticket_ready).await;
    }
    if args.ftp {
        return send_ftp(args, show_progress, &ticket_ready).await;
    }
    if args.sftp {
        return send_sftp(args, show_progress, &ticket_ready).await;
    }

    let source = Source::open(args.path.clone(), args.name.clone()).await?;
    let endpoint = bind_endpoint(endpoint_policy_for_send(&args)?, FILE_ALPN).await?;

    if should_wait_online(&args) {
        endpoint.online().await;
    }

    let endpoint_addr = endpoint.addr();
    let ticket = match &args.relay {
        Some(relay_url) if args.accept_self_signed_relay => Ticket::relay_only(
            iroh::EndpointAddr::from_parts(
                endpoint_addr.id,
                [TransportAddr::Relay(relay_url.clone())],
            ),
            source.name().to_string(),
            source.kind(),
            source.size(),
            source.content_md5(),
        ),
        Some(relay_url) => Ticket::trusted_relay_only(
            iroh::EndpointAddr::from_parts(
                endpoint_addr.id,
                [TransportAddr::Relay(relay_url.clone())],
            ),
            source.name().to_string(),
            source.kind(),
            source.size(),
            source.content_md5(),
        ),
        None => Ticket::peer(
            endpoint_addr,
            source.name().to_string(),
            source.kind(),
            source.size(),
            source.content_md5(),
        ),
    };
    let ticket_str = ticket.encode()?;
    ticket_ready(&ticket_str)?;

    let mut accepted = 0usize;
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            incoming = endpoint.accept() => {
                let Some(incoming) = incoming else {
                    break;
                };
                let conn = match incoming.accept() {
                    Ok(conn) => conn,
                    Err(err) => {
                        eprintln!("ii send: dropped incoming connection: {err:#}");
                        continue;
                    }
                };
                let conn = match conn.await {
                    Ok(conn) => conn,
                    Err(err) => {
                        eprintln!("ii send: failed to accept connection: {err:#}");
                        continue;
                    }
                };
                match serve_one(conn, &source, show_progress).await {
                    Ok(ServeOutcome::Sent) => {
                        accepted += 1;
                        if !args.keep_alive {
                            break;
                        }
                    }
                    Ok(ServeOutcome::Ignored) => {}
                    Err(err) => eprintln!("ii send: transfer failed: {err:#}"),
                }
            }
        }
    }

    endpoint.close().await;
    if accepted == 0 {
        eprintln!("ii send: no receiver connected");
    }
    Ok(())
}

async fn send_s3<F>(args: SendArgs, show_progress: bool, ticket_ready: &F) -> Result<()>
where
    F: Fn(&str) -> Result<()> + Send + Sync,
{
    let selection = match args.profile.as_deref() {
        Some(profile) => storage::load_or_prompt_s3_profile_named(profile)?,
        None => storage::load_or_prompt_s3_profile()?,
    };
    let source = Source::open(args.path.clone(), args.name.clone()).await?;
    let upload = crate::backend::s3::upload(
        &source,
        &selection.profile,
        args.delete_after_recv,
        show_progress,
    )
    .await?;
    if selection.save_after_success {
        storage::save_config(&selection.path, &selection.config)?;
    }
    let ticket = Ticket::s3(
        upload.download_url,
        upload.delete_url,
        upload.object_key,
        source.name().to_string(),
        source.kind(),
        source.size(),
        source.content_md5(),
    );
    let ticket_str = ticket.encode()?;
    ticket_ready(&ticket_str)?;
    Ok(())
}

async fn send_webdav<F>(args: SendArgs, show_progress: bool, ticket_ready: &F) -> Result<()>
where
    F: Fn(&str) -> Result<()> + Send + Sync,
{
    let selection = match args.profile.as_deref() {
        Some(profile) => storage::load_or_prompt_webdav_profile_named(profile)?,
        None => storage::load_or_prompt_webdav_profile()?,
    };
    let source = Source::open(args.path.clone(), args.name.clone()).await?;
    let upload = crate::backend::webdav::upload(&source, &selection.profile, show_progress).await?;
    if selection.save_after_success {
        storage::save_config(&selection.path, &selection.config)?;
    }
    let portable = if args.portable_webdav {
        eprintln!("ii send: warning: portable WebDAV ticket includes URL, username, and password");
        Some(WebDavPortableCredentials {
            url: selection.profile.url.clone(),
            username: selection.profile.username.clone(),
            password: selection.profile.password.clone(),
            auth: webdav_auth_name(&selection.profile.auth).to_string(),
        })
    } else {
        None
    };
    let ticket = Ticket::webdav(
        selection.profile_name,
        upload.object_key,
        args.delete_after_recv,
        portable,
        source.name().to_string(),
        source.kind(),
        source.size(),
        source.content_md5(),
    );
    let ticket_str = ticket.encode()?;
    ticket_ready(&ticket_str)?;
    Ok(())
}

fn webdav_auth_name(auth: &storage::WebDavAuth) -> &'static str {
    match auth {
        storage::WebDavAuth::Basic => "basic",
        storage::WebDavAuth::Digest => "digest",
    }
}

async fn send_ftp<F>(args: SendArgs, show_progress: bool, ticket_ready: &F) -> Result<()>
where
    F: Fn(&str) -> Result<()> + Send + Sync,
{
    let selection = match args.profile.as_deref() {
        Some(profile) => storage::load_or_prompt_ftp_profile_named(profile)?,
        None => storage::load_or_prompt_ftp_profile()?,
    };
    let source = Source::open(args.path.clone(), args.name.clone()).await?;
    let upload = crate::backend::ftp::upload(&source, &selection.profile, show_progress).await?;
    if selection.save_after_success {
        storage::save_config(&selection.path, &selection.config)?;
    }
    let portable = if args.portable_webdav {
        eprintln!("ii send: warning: portable FTP ticket includes URL, username, and password");
        Some(FtpPortableCredentials {
            url: selection.profile.url.clone(),
            username: selection.profile.username.clone(),
            password: selection.profile.password.clone(),
            remote_dir: selection.profile.remote_dir.clone(),
        })
    } else {
        None
    };
    let ticket = Ticket::ftp(
        selection.profile_name,
        upload.object_key,
        args.delete_after_recv,
        portable,
        source.name().to_string(),
        source.kind(),
        source.size(),
        source.content_md5(),
    );
    ticket_ready(&ticket.encode()?)
}

async fn send_sftp<F>(args: SendArgs, show_progress: bool, ticket_ready: &F) -> Result<()>
where
    F: Fn(&str) -> Result<()> + Send + Sync,
{
    let selection = match args.profile.as_deref() {
        Some(profile) => storage::load_or_prompt_sftp_profile_named(profile)?,
        None => storage::load_or_prompt_sftp_profile()?,
    };
    let source = Source::open(args.path.clone(), args.name.clone()).await?;
    let upload = crate::backend::sftp::upload(&source, &selection.profile, show_progress).await?;
    if selection.save_after_success {
        storage::save_config(&selection.path, &selection.config)?;
    }
    let portable = if args.portable_webdav {
        eprintln!("ii send: warning: portable SFTP ticket includes credentials or a private key");
        Some(crate::backend::sftp::portable_credentials(
            &selection.profile,
        )?)
    } else {
        None
    };
    let ticket = Ticket::sftp(
        selection.profile_name,
        upload.object_key,
        args.delete_after_recv,
        portable,
        source.name().to_string(),
        source.kind(),
        source.size(),
        source.content_md5(),
    );
    ticket_ready(&ticket.encode()?)
}

fn print_ticket(ticket: &str, copy: bool, output: Option<PathBuf>) -> Result<()> {
    let recv_command = format!("ii recv {ticket}");
    println!("ii ticket:");
    println!("{ticket}");
    println!();
    println!("on the other computer:");
    println!("{recv_command}");
    if let Some(path) = output {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create output dir {}", parent.display()))?;
        }
        std::fs::write(&path, format!("{recv_command}\n"))
            .with_context(|| format!("write recv command {}", path.display()))?;
    }
    if copy && maybe_copy_recv_command(&recv_command)? {
        println!();
        println!("recv command copied to clipboard");
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn maybe_copy_recv_command(command: &str) -> Result<bool> {
    copy_text_to_clipboard(command).map(|_| true)
}

#[cfg(not(target_os = "windows"))]
fn maybe_copy_recv_command(command: &str) -> Result<bool> {
    copy_text_to_clipboard(command).map(|_| true)
}

#[cfg(target_os = "windows")]
fn copy_text_to_clipboard(text: &str) -> Result<()> {
    let mut child = Command::new("clip")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("start clip.exe")?;
    {
        let stdin = child.stdin.as_mut().context("open clip.exe stdin")?;
        stdin.write_all(text.as_bytes()).context("write clip.exe")?;
    }
    let status = child.wait().context("wait clip.exe")?;
    if !status.success() {
        bail!("clip.exe exited with {status}");
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn copy_text_to_clipboard(text: &str) -> Result<()> {
    let mut child = Command::new("pbcopy")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("start pbcopy")?;
    {
        let stdin = child.stdin.as_mut().context("open pbcopy stdin")?;
        stdin.write_all(text.as_bytes()).context("write pbcopy")?;
    }
    let status = child.wait().context("wait pbcopy")?;
    if !status.success() {
        bail!("pbcopy exited with {status}");
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn copy_text_to_clipboard(text: &str) -> Result<()> {
    for command in ["wl-copy", "xclip", "xsel"] {
        if let Ok(()) = try_copy_with_command(command, text) {
            return Ok(());
        }
    }
    bail!("no clipboard tool found; install wl-copy, xclip, or xsel");
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
fn copy_text_to_clipboard(_text: &str) -> Result<()> {
    bail!("clipboard copy is not supported on this platform")
}

#[cfg(target_os = "linux")]
fn try_copy_with_command(command: &str, text: &str) -> Result<()> {
    let mut cmd = Command::new(command);
    if command == "xclip" {
        cmd.args(["-selection", "clipboard"]);
    } else if command == "xsel" {
        cmd.args(["--clipboard", "--input"]);
    }
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("start {command}"))?;
    {
        let stdin = child.stdin.as_mut().context("open clipboard stdin")?;
        stdin
            .write_all(text.as_bytes())
            .with_context(|| format!("write {command}"))?;
    }
    let status = child.wait().with_context(|| format!("wait {command}"))?;
    if !status.success() {
        bail!("{command} exited with {status}");
    }
    Ok(())
}
