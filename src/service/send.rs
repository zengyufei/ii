use crate::{
    command::SendArgs,
    service::{TransferEvent, web::send_web},
    storage,
    ticket::{FtpPortableCredentials, Ticket, WebDavPortableCredentials},
    transport::{
        iroh::{FILE_ALPN, bind_endpoint, endpoint_policy_for_send, should_wait_online},
        p2p::{ServeOutcome, serve_one_limited},
        progress::{RateLimiter, should_show_progress},
        source::Source,
    },
};
use anyhow::{Context, Result, bail};
use iroh::TransportAddr;
use std::{
    collections::VecDeque,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
    sync::Arc,
};
use tokio::task::JoinSet;

const MAX_ACTIVE_TRANSFERS: usize = 16;
const MAX_QUEUED_TRANSFERS: usize = 1_000;

#[derive(Clone, Copy)]
struct TransferLimits {
    active: usize,
    queued: usize,
}

const DEFAULT_TRANSFER_LIMITS: TransferLimits = TransferLimits {
    active: MAX_ACTIVE_TRANSFERS,
    queued: MAX_QUEUED_TRANSFERS,
};

async fn source_from_args(args: &SendArgs) -> Result<Source> {
    Source::open_paths(
        args.path.clone(),
        &args.extra_paths,
        args.name.clone(),
        &args.include,
        &args.exclude,
    )
    .await
}

struct QueuedConnection {
    number: usize,
    conn: iroh::endpoint::Connection,
}

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
    let json = args.json;
    if json {
        crate::json::started("send");
    }
    if args.web {
        let result = send_web(args).await;
        if result.is_ok() && json {
            crate::json::completed("send");
        }
        return result;
    }
    let copy = args.copy;
    let output = args.output.clone();
    let result = send_inner(args, move |ticket| {
        if json {
            crate::json::emit("ticket", &[("ticket", crate::json::Value::String(ticket))]);
            crate::json::progress("send", 0);
            Ok(())
        } else {
            print_ticket(ticket, copy, output.clone())
        }
    })
    .await;
    if result.is_ok() && json {
        crate::json::completed("send");
    }
    result
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
    let show_progress = !args.json && should_show_progress(false);
    if args.delete_after_recv
        && !args.s3
        && !args.r2
        && !args.azure
        && !args.webdav
        && !args.ftp
        && !args.sftp
    {
        bail!("-d requires --s3, --r2, --azure, --webdav, --ftp or --sftp");
    }
    if args.profile.is_some()
        && !args.s3
        && !args.r2
        && !args.azure
        && !args.webdav
        && !args.ftp
        && !args.sftp
    {
        bail!("--profile requires --s3, --r2, --azure, --webdav, --ftp or --sftp");
    }
    if args.s3 {
        return send_s3(args, show_progress, &ticket_ready).await;
    }
    if args.r2 {
        return send_r2(args, show_progress, &ticket_ready).await;
    }
    if args.azure {
        return send_azure(args, show_progress, &ticket_ready).await;
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

    let source = source_from_args(&args).await?;
    let rate_limiter = args.rate.map(RateLimiter::new).map(Arc::new);
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

    let _advertiser = if args.keep_alive {
        Some(
            crate::discovery::advertise(crate::discovery::Service::Send {
                ticket: ticket_str.clone(),
                name: source.name().to_string(),
                kind: match source.kind() {
                    crate::ticket::PayloadKind::File => "file".to_string(),
                    crate::ticket::PayloadKind::Dir => "dir".to_string(),
                    crate::ticket::PayloadKind::Stdin => "stdin".to_string(),
                },
                size: source.size(),
            })
            .await?,
        )
    } else {
        None
    };

    if args.keep_alive {
        let accepted =
            serve_keep_alive(&endpoint, Arc::new(source), show_progress, rate_limiter).await;
        if accepted == 0 {
            eprintln!("ii send: no receiver connected");
        }
        return Ok(());
    }

    let accepted = serve_once(&endpoint, &source, show_progress, rate_limiter).await;

    endpoint.close().await;
    if accepted == 0 {
        eprintln!("ii send: no receiver connected");
    }
    Ok(())
}

async fn serve_once(
    endpoint: &iroh::Endpoint,
    source: &Source,
    show_progress: bool,
    rate_limiter: Option<Arc<RateLimiter>>,
) -> usize {
    let mut accepted = 0;
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
                match serve_one_limited(conn, source, show_progress, rate_limiter.clone()).await {
                    Ok(ServeOutcome::Sent) => {
                        accepted += 1;
                        break;
                    }
                    Ok(ServeOutcome::Ignored) => {}
                    Err(err) => eprintln!("ii send: transfer failed: {err:#}"),
                }
            }
        }
    }

    accepted
}

async fn serve_keep_alive(
    endpoint: &iroh::Endpoint,
    source: Arc<Source>,
    show_progress: bool,
    rate_limiter: Option<Arc<RateLimiter>>,
) -> usize {
    serve_keep_alive_with_limits_and_rate(
        endpoint,
        source,
        show_progress,
        DEFAULT_TRANSFER_LIMITS,
        rate_limiter,
    )
    .await
}

#[cfg(test)]
async fn serve_keep_alive_with_limits(
    endpoint: &iroh::Endpoint,
    source: Arc<Source>,
    show_progress: bool,
    limits: TransferLimits,
) -> usize {
    serve_keep_alive_with_limits_and_rate(endpoint, source, show_progress, limits, None).await
}

async fn serve_keep_alive_with_limits_and_rate(
    endpoint: &iroh::Endpoint,
    source: Arc<Source>,
    show_progress: bool,
    limits: TransferLimits,
    rate_limiter: Option<Arc<RateLimiter>>,
) -> usize {
    let mut accepted = 0;
    let mut next_number = 1;
    let mut waiting = VecDeque::new();
    let mut handshakes = JoinSet::new();
    let mut transfers = JoinSet::new();

    loop {
        start_queued_transfers(
            &mut transfers,
            &mut waiting,
            &source,
            show_progress,
            limits.active,
            rate_limiter.as_ref(),
        );

        tokio::select! {
            biased;
            _ = tokio::signal::ctrl_c() => break,
            joined = transfers.join_next(), if !transfers.is_empty() => {
                match joined {
                    Some(Ok(Ok(ServeOutcome::Sent))) => accepted += 1,
                    Some(Ok(Ok(ServeOutcome::Ignored))) | Some(Err(_)) | None => {}
                    Some(Ok(Err(_))) => {}
                }
            }
            joined = handshakes.join_next(), if !handshakes.is_empty() => {
                match joined {
                    Some(Ok(Ok(conn))) => {
                        let queued = QueuedConnection { number: next_number, conn };
                        next_number += 1;
                        if transfers.len() < limits.active {
                            start_transfer(
                                &mut transfers,
                                queued,
                                Arc::clone(&source),
                                show_progress,
                                rate_limiter.clone(),
                            );
                        } else {
                            waiting.push_back(queued);
                        }
                    }
                    Some(Ok(Err(err))) => {
                        eprintln!("ii send: failed to accept connection: {err:#}");
                    }
                    Some(Err(err)) => {
                        eprintln!("ii send: connection task failed: {err}");
                    }
                    None => {}
                }
            }
            incoming = endpoint.accept() => {
                let Some(incoming) = incoming else {
                    break;
                };
                if !has_capacity(transfers.len(), waiting.len(), handshakes.len(), limits) {
                    incoming.refuse();
                    continue;
                }
                handshakes.spawn(async move {
                    let accepting = incoming.accept().context("accept incoming connection")?;
                    accepting.await.context("complete incoming connection")
                });
            }
        }
    }

    endpoint.close().await;
    waiting.clear();
    handshakes.abort_all();
    transfers.abort_all();
    while handshakes.join_next().await.is_some() {}
    while transfers.join_next().await.is_some() {}
    accepted
}

fn has_capacity(active: usize, waiting: usize, handshakes: usize, limits: TransferLimits) -> bool {
    active + waiting + handshakes < limits.active + limits.queued
}

fn start_queued_transfers(
    transfers: &mut JoinSet<Result<ServeOutcome>>,
    waiting: &mut VecDeque<QueuedConnection>,
    source: &Arc<Source>,
    show_progress: bool,
    max_active: usize,
    rate_limiter: Option<&Arc<RateLimiter>>,
) {
    while transfers.len() < max_active {
        let Some(queued) = waiting.pop_front() else {
            break;
        };
        start_transfer(
            transfers,
            queued,
            Arc::clone(source),
            show_progress,
            rate_limiter.cloned(),
        );
    }
}

fn start_transfer(
    transfers: &mut JoinSet<Result<ServeOutcome>>,
    queued: QueuedConnection,
    source: Arc<Source>,
    show_progress: bool,
    rate_limiter: Option<Arc<RateLimiter>>,
) {
    transfers.spawn(async move {
        let result = crate::transport::p2p::serve_one_multiline_limited(
            queued.conn,
            source.as_ref(),
            show_progress,
            format!("ii send #{}", queued.number),
            rate_limiter,
        )
        .await;
        if let Err(err) = &result {
            eprintln!("ii send #{}: transfer failed: {err:#}", queued.number);
        }
        result
    });
}

async fn send_s3<F>(args: SendArgs, show_progress: bool, ticket_ready: &F) -> Result<()>
where
    F: Fn(&str) -> Result<()> + Send + Sync,
{
    let selection = if args.json {
        storage::load_s3_profile_noninteractive(args.profile.as_deref())?
    } else {
        match args.profile.as_deref() {
            Some(profile) => storage::load_or_prompt_s3_profile_named(profile)?,
            None => storage::load_or_prompt_s3_profile()?,
        }
    };
    let source = source_from_args(&args).await?;
    let upload = crate::backend::s3::upload(
        &source,
        &selection.profile,
        args.delete_after_recv,
        show_progress,
        args.rate.map(RateLimiter::new).map(Arc::new),
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

async fn send_r2<F>(args: SendArgs, show_progress: bool, ticket_ready: &F) -> Result<()>
where
    F: Fn(&str) -> Result<()> + Send + Sync,
{
    let selection = if args.json {
        storage::load_r2_profile_noninteractive(args.profile.as_deref())?
    } else {
        match args.profile.as_deref() {
            Some(profile) => storage::load_or_prompt_r2_profile_named(profile)?,
            None => storage::load_or_prompt_r2_profile()?,
        }
    };
    let source = source_from_args(&args).await?;
    let profile = storage::r2_as_s3_profile(&selection.profile);
    let upload = crate::backend::s3::upload(
        &source,
        &profile,
        args.delete_after_recv,
        show_progress,
        args.rate.map(RateLimiter::new).map(Arc::new),
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
    ticket_ready(&ticket.encode()?)
}

async fn send_azure<F>(args: SendArgs, show_progress: bool, ticket_ready: &F) -> Result<()>
where
    F: Fn(&str) -> Result<()> + Send + Sync,
{
    let selection = if args.json {
        storage::load_azure_profile_noninteractive(args.profile.as_deref())?
    } else {
        match args.profile.as_deref() {
            Some(profile) => storage::load_or_prompt_azure_profile_named(profile)?,
            None => storage::load_or_prompt_azure_profile()?,
        }
    };
    if args.delete_after_recv {
        storage::validate_azure_delete_permission(&selection.profile)?;
    }
    if selection.profile.auth == storage::AzureAuth::Sas {
        eprintln!(
            "ii send: warning: Azure SAS ticket includes the SAS token and its full permissions"
        );
    }
    let source = source_from_args(&args).await?;
    let upload = crate::backend::azure::upload(
        &source,
        &selection.profile,
        args.delete_after_recv,
        show_progress,
        args.rate.map(RateLimiter::new).map(Arc::new),
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
    ticket_ready(&ticket.encode()?)
}

async fn send_webdav<F>(args: SendArgs, show_progress: bool, ticket_ready: &F) -> Result<()>
where
    F: Fn(&str) -> Result<()> + Send + Sync,
{
    let selection = if args.json {
        storage::load_webdav_profile_noninteractive(args.profile.as_deref().unwrap_or("default"))?
    } else {
        match args.profile.as_deref() {
            Some(profile) => storage::load_or_prompt_webdav_profile_named(profile)?,
            None => storage::load_or_prompt_webdav_profile()?,
        }
    };
    let source = source_from_args(&args).await?;
    let upload = crate::backend::webdav::upload(
        &source,
        &selection.profile,
        show_progress,
        args.rate.map(RateLimiter::new).map(Arc::new),
    )
    .await?;
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
    let selection = if args.json {
        storage::load_ftp_profile_noninteractive(args.profile.as_deref())?
    } else {
        match args.profile.as_deref() {
            Some(profile) => storage::load_or_prompt_ftp_profile_named(profile)?,
            None => storage::load_or_prompt_ftp_profile()?,
        }
    };
    let source = source_from_args(&args).await?;
    let upload = crate::backend::ftp::upload(
        &source,
        &selection.profile,
        show_progress,
        args.rate.map(RateLimiter::new).map(Arc::new),
    )
    .await?;
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
    let selection = if args.json {
        storage::load_sftp_profile_noninteractive(args.profile.as_deref())?
    } else {
        match args.profile.as_deref() {
            Some(profile) => storage::load_or_prompt_sftp_profile_named(profile)?,
            None => storage::load_or_prompt_sftp_profile()?,
        }
    };
    let source = source_from_args(&args).await?;
    let upload = crate::backend::sftp::upload(
        &source,
        &selection.profile,
        show_progress,
        args.rate.map(RateLimiter::new).map(Arc::new),
    )
    .await?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        command::RelayArgs,
        relay::build_server_config,
        transport::iroh::{EndpointPolicy, FILE_ALPN, bind_endpoint},
    };
    use iroh::RelayMode;
    use tokio::time::{Duration, timeout};

    async fn test_endpoint() -> iroh::Endpoint {
        bind_endpoint(EndpointPolicy::standard(RelayMode::Disabled), FILE_ALPN)
            .await
            .unwrap()
    }

    async fn relay_endpoint(relay_url: iroh::RelayUrl) -> iroh::Endpoint {
        bind_endpoint(EndpointPolicy::TrustedRelayOnly(relay_url), FILE_ALPN)
            .await
            .unwrap()
    }

    async fn test_relay() -> (iroh_relay::server::Server, iroh::RelayUrl) {
        let relay = iroh_relay::server::Server::spawn(
            build_server_config(&RelayArgs {
                tls: false,
                domain: None,
                cert: None,
                key: None,
                port: None,
                bind: None,
            })
            .unwrap(),
        )
        .await
        .unwrap();
        let relay_url = format!("http://127.0.0.1:{}", relay.http_addr().unwrap().port())
            .parse()
            .unwrap();
        (relay, relay_url)
    }

    #[test]
    fn capacity_counts_handshakes_and_queue() {
        let limits = TransferLimits {
            active: 2,
            queued: 3,
        };
        assert!(has_capacity(2, 2, 0, limits));
        assert!(!has_capacity(2, 2, 1, limits));
    }

    #[tokio::test]
    async fn queued_receiver_starts_after_stalled_receiver_closes() {
        crate::install_crypto_provider();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source.bin");
        std::fs::write(&path, b"queued transfer").unwrap();
        let source = Arc::new(Source::from_file(path, None).await.unwrap());
        let sender = test_endpoint().await;
        let sender_for_task = sender.clone();
        let sender_task = tokio::spawn(async move {
            serve_keep_alive_with_limits(
                &sender_for_task,
                Arc::clone(&source),
                false,
                TransferLimits {
                    active: 1,
                    queued: 1,
                },
            )
            .await
        });
        let first = test_endpoint().await;
        let first_conn = timeout(
            Duration::from_secs(5),
            first.connect(sender.addr(), FILE_ALPN),
        )
        .await
        .unwrap()
        .unwrap();
        let (_first_send, _first_recv) = first_conn.open_bi().await.unwrap();

        let second = test_endpoint().await;
        let second_conn = timeout(
            Duration::from_secs(5),
            second.connect(sender.addr(), FILE_ALPN),
        )
        .await
        .unwrap()
        .unwrap();
        let (mut second_send, mut second_recv) = second_conn.open_bi().await.unwrap();
        second_send
            .write_all(
                &postcard::to_stdvec(&crate::ticket::ResumeRequest { resume_from: 0 }).unwrap(),
            )
            .await
            .unwrap();
        second_send.finish().unwrap();

        let third = test_endpoint().await;
        assert!(
            timeout(
                Duration::from_secs(5),
                third.connect(sender.addr(), FILE_ALPN),
            )
            .await
            .unwrap()
            .is_err()
        );

        first_conn.close(0u32.into(), b"cancel");
        let bytes = timeout(Duration::from_secs(5), second_recv.read_to_end(1024))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(bytes, b"queued transfer");

        second_conn.close(0u32.into(), b"done");
        sender_task.abort();
        let _ = sender_task.await;
        sender.close().await;
        first.close().await;
        second.close().await;
        third.close().await;
    }

    #[tokio::test]
    async fn waiting_connections_are_started_fifo() {
        crate::install_crypto_provider();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source.bin");
        std::fs::write(&path, b"fifo transfer").unwrap();
        let source = Arc::new(Source::from_file(path, None).await.unwrap());
        let sender = test_endpoint().await;
        let sender_for_task = sender.clone();
        let sender_task = tokio::spawn(async move {
            serve_keep_alive_with_limits(
                &sender_for_task,
                source,
                false,
                TransferLimits {
                    active: 1,
                    queued: 2,
                },
            )
            .await
        });

        let first = test_endpoint().await;
        let first_conn = timeout(
            Duration::from_secs(5),
            first.connect(sender.addr(), FILE_ALPN),
        )
        .await
        .unwrap()
        .unwrap();
        let (_first_send, _first_recv) = first_conn.open_bi().await.unwrap();

        let second = test_endpoint().await;
        let second_conn = timeout(
            Duration::from_secs(5),
            second.connect(sender.addr(), FILE_ALPN),
        )
        .await
        .unwrap()
        .unwrap();
        let (mut second_send, mut second_recv) = second_conn.open_bi().await.unwrap();
        second_send
            .write_all(
                &postcard::to_stdvec(&crate::ticket::ResumeRequest { resume_from: 0 }).unwrap(),
            )
            .await
            .unwrap();
        second_send.finish().unwrap();

        let third = test_endpoint().await;
        let third_conn = timeout(
            Duration::from_secs(5),
            third.connect(sender.addr(), FILE_ALPN),
        )
        .await
        .unwrap()
        .unwrap();
        let (mut third_send, mut third_recv) = third_conn.open_bi().await.unwrap();
        third_send
            .write_all(
                &postcard::to_stdvec(&crate::ticket::ResumeRequest { resume_from: 0 }).unwrap(),
            )
            .await
            .unwrap();
        third_send.finish().unwrap();

        first_conn.close(0u32.into(), b"cancel");
        let second_bytes = timeout(Duration::from_secs(5), second_recv.read_to_end(1024))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(second_bytes, b"fifo transfer");
        assert!(
            timeout(Duration::from_millis(250), third_recv.read_to_end(1024))
                .await
                .is_err()
        );

        second_conn.close(0u32.into(), b"done");
        let third_bytes = timeout(Duration::from_secs(5), third_recv.read_to_end(1024))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(third_bytes, b"fifo transfer");

        third_conn.close(0u32.into(), b"done");
        sender_task.abort();
        let _ = sender_task.await;
        sender.close().await;
        first.close().await;
        second.close().await;
        third.close().await;
    }

    #[tokio::test]
    async fn stalled_receiver_does_not_block_another_active_transfer() {
        crate::install_crypto_provider();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source.bin");
        std::fs::write(&path, b"parallel transfer").unwrap();
        let source = Arc::new(Source::from_file(path, None).await.unwrap());
        let (relay, relay_url) = test_relay().await;
        let sender = relay_endpoint(relay_url.clone()).await;
        timeout(Duration::from_secs(5), sender.online())
            .await
            .unwrap();
        let sender_for_task = sender.clone();
        let sender_task = tokio::spawn(async move {
            serve_keep_alive_with_limits(
                &sender_for_task,
                source,
                false,
                TransferLimits {
                    active: 2,
                    queued: 0,
                },
            )
            .await
        });

        let stalled = relay_endpoint(relay_url.clone()).await;
        timeout(Duration::from_secs(5), stalled.online())
            .await
            .unwrap();
        let stalled_conn = timeout(
            Duration::from_secs(5),
            stalled.connect(sender.addr(), FILE_ALPN),
        )
        .await
        .unwrap()
        .unwrap();
        let (_stalled_send, _stalled_recv) = stalled_conn.open_bi().await.unwrap();

        let receiver = relay_endpoint(relay_url).await;
        timeout(Duration::from_secs(5), receiver.online())
            .await
            .unwrap();
        let conn = timeout(
            Duration::from_secs(5),
            receiver.connect(sender.addr(), FILE_ALPN),
        )
        .await
        .unwrap()
        .unwrap();
        let (mut send, mut recv) = conn.open_bi().await.unwrap();
        send.write_all(
            &postcard::to_stdvec(&crate::ticket::ResumeRequest { resume_from: 0 }).unwrap(),
        )
        .await
        .unwrap();
        send.finish().unwrap();

        let bytes = timeout(Duration::from_secs(5), recv.read_to_end(1024))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(bytes, b"parallel transfer");

        conn.close(0u32.into(), b"done");
        stalled_conn.close(0u32.into(), b"cancel");
        sender_task.abort();
        let _ = sender_task.await;
        sender.close().await;
        stalled.close().await;
        receiver.close().await;
        relay.shutdown().await.unwrap();
    }
}
