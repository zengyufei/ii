use crate::{
    command::RecvArgs,
    ticket::{PayloadKind, ResumeRequest, Ticket},
    transport::{
        progress::{TransferProgress, copy_with_progress, fmt_bytes, fmt_duration},
        source::{Source, md5_path},
    },
};
use anyhow::{Context, Result, bail};
use iroh::Endpoint;
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tokio::{
    fs,
    io::{self, AsyncWriteExt},
};

const DEFAULT_CONNECT_FAST_PATH_TIMEOUT: Duration = Duration::from_secs(3);

pub(crate) async fn write_to_file(
    mut recv: iroh::endpoint::RecvStream,
    path: PathBuf,
    resume_from: u64,
    total_size: Option<u64>,
    show_progress: bool,
) -> Result<u64> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await.ok();
    }
    let mut file = if resume_from > 0 {
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await
            .with_context(|| format!("open destination {}", path.display()))?
    } else {
        fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&path)
            .await
            .with_context(|| format!("open destination {}", path.display()))?
    };
    let mut progress = TransferProgress::new("ii recv", show_progress, total_size, resume_from);
    let bytes = copy_with_progress(&mut recv, &mut file, &mut progress)
        .await
        .with_context(|| format!("write destination {}", path.display()))?;
    progress.finish();
    file.flush()
        .await
        .with_context(|| format!("flush destination {}", path.display()))?;
    Ok(bytes)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FilePlan {
    Download { resume_from: u64 },
    Skip,
}

pub(crate) struct RecvTrace {
    enabled: bool,
    started: Instant,
    last: Instant,
}

impl RecvTrace {
    pub(crate) fn new(enabled: bool) -> Self {
        let now = Instant::now();
        Self {
            enabled,
            started: now,
            last: now,
        }
    }

    pub(crate) fn info(&self, message: impl std::fmt::Display) {
        if self.enabled {
            eprintln!("ii recv trace: {message}");
        }
    }

    pub(crate) fn step(&mut self, label: &str) {
        if !self.enabled {
            return;
        }
        let now = Instant::now();
        eprintln!(
            "ii recv trace: {label}: +{} total {}",
            fmt_duration(now.duration_since(self.last)),
            fmt_duration(now.duration_since(self.started))
        );
        self.last = now;
    }

    pub(crate) fn finish(&self, bytes: u64) {
        if !self.enabled {
            return;
        }
        let elapsed = self.started.elapsed();
        let seconds = elapsed.as_secs_f64();
        let mib_per_second = if seconds > 0.0 {
            bytes as f64 / 1024.0 / 1024.0 / seconds
        } else {
            0.0
        };
        eprintln!(
            "ii recv trace: done: {} in {}, {:.2} MiB/s",
            fmt_bytes(bytes),
            fmt_duration(elapsed),
            mib_per_second
        );
    }
}

pub(crate) fn trace_endpoint_addr(label: &str, addr: &iroh::EndpointAddr, trace: &RecvTrace) {
    if !trace.enabled {
        return;
    }
    let ip_addrs = addr.ip_addrs().map(ToString::to_string).collect::<Vec<_>>();
    let relay_urls = addr
        .relay_urls()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    trace.info(format_args!(
        "{label}: total={}, ip={}, relay={}",
        addr.addrs.len(),
        ip_addrs.len(),
        relay_urls.len()
    ));
    if !ip_addrs.is_empty() {
        trace.info(format_args!("{label} ip: {}", ip_addrs.join(", ")));
    }
    if !relay_urls.is_empty() {
        trace.info(format_args!("{label} relay: {}", relay_urls.join(", ")));
    }
}

pub(crate) fn payload_kind_name(kind: PayloadKind) -> &'static str {
    match kind {
        PayloadKind::File => "file",
        PayloadKind::Dir => "dir",
        PayloadKind::Stdin => "stdin",
    }
}

pub(crate) async fn copy_to_stdout(
    mut recv: iroh::endpoint::RecvStream,
    total_size: Option<u64>,
    show_progress: bool,
) -> Result<u64> {
    let mut stdout = io::stdout();
    let mut progress = TransferProgress::new("ii recv", show_progress, total_size, 0);
    let bytes = copy_with_progress(&mut recv, &mut stdout, &mut progress)
        .await
        .context("write stdout")?;
    progress.finish();
    stdout.flush().await.ok();
    Ok(bytes)
}

pub(crate) async fn connect_to_peer(
    endpoint: &Endpoint,
    endpoint_addr: iroh::EndpointAddr,
    local_only: bool,
    alpn: &[u8],
    trace: &RecvTrace,
) -> Result<iroh::endpoint::Connection> {
    if local_only {
        trace.info("connecting to sender");
        return endpoint
            .connect(endpoint_addr, alpn)
            .await
            .context("connect to sender");
    }

    let relay_only = relay_only_addr(&endpoint_addr);
    if relay_only.is_none() {
        trace.info("connecting to sender");
        return endpoint
            .connect(endpoint_addr, alpn)
            .await
            .context("connect to sender");
    }

    trace.info(format_args!(
        "connecting to sender, full address set gets {} before relay-only fallback",
        fmt_duration(DEFAULT_CONNECT_FAST_PATH_TIMEOUT)
    ));
    match tokio::time::timeout(
        DEFAULT_CONNECT_FAST_PATH_TIMEOUT,
        endpoint.connect(endpoint_addr, alpn),
    )
    .await
    {
        Ok(result) => result.context("connect to sender"),
        Err(_) => {
            let relay_only = relay_only.expect("checked above");
            trace.info("full address connect timed out; retrying relay-only");
            trace_endpoint_addr("relay-only endpoints", &relay_only, trace);
            endpoint
                .connect(relay_only, alpn)
                .await
                .context("connect to sender via relay")
        }
    }
}

pub(crate) fn relay_only_addr(addr: &iroh::EndpointAddr) -> Option<iroh::EndpointAddr> {
    let addrs = addr
        .addrs
        .iter()
        .filter(|addr| addr.is_relay())
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    (!addrs.is_empty()).then(|| iroh::EndpointAddr { id: addr.id, addrs })
}

pub(crate) async fn plan_file_receive(
    args: &RecvArgs,
    ticket: &Ticket,
    path: &Path,
    trace: &RecvTrace,
) -> Result<FilePlan> {
    if args.overwrite {
        trace.info(format_args!("overwrite requested for {}", path.display()));
        return Ok(FilePlan::Download { resume_from: 0 });
    }
    if args.resume {
        if !matches!(ticket.kind(), PayloadKind::File | PayloadKind::Stdin) {
            bail!("--resume is only supported for regular files");
        }
        let resume_from = existing_size(path)?;
        trace.info(format_args!("explicit resume from byte {}", resume_from));
        return Ok(FilePlan::Download { resume_from });
    }
    if !path.exists() {
        trace.info(format_args!("fresh download to {}", path.display()));
        return Ok(FilePlan::Download { resume_from: 0 });
    }
    if path.is_dir() {
        bail!("destination exists but is a directory: {}", path.display());
    }

    let existing_size = existing_size(path)?;
    let ticket_size = ticket.size();
    if let Some(expected_hash) = ticket.content_md5() {
        if ticket_size == Some(existing_size) {
            let actual_hash = md5_path(path.to_path_buf()).await?;
            if actual_hash == expected_hash {
                return Ok(FilePlan::Skip);
            }
        }
    }

    if let Some(size) = ticket_size {
        if existing_size < size {
            trace.info(format_args!(
                "auto resume {} from byte {}",
                path.display(),
                existing_size
            ));
            return Ok(FilePlan::Download {
                resume_from: existing_size,
            });
        }
    }

    trace.info(format_args!("overwrite existing file {}", path.display()));
    Ok(FilePlan::Download { resume_from: 0 })
}

fn existing_size(path: &Path) -> Result<u64> {
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_file() => Ok(meta.len()),
        Ok(_) => bail!("destination exists but is not a file: {}", path.display()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(err) => Err(err).with_context(|| format!("stat existing file {}", path.display())),
    }
}

pub(crate) enum ServeOutcome {
    Sent,
    Ignored,
}

pub(crate) async fn serve_one(
    conn: iroh::endpoint::Connection,
    source: &Source,
    show_progress: bool,
) -> Result<ServeOutcome> {
    serve_one_inner(conn, source, show_progress, None).await
}

pub(crate) async fn serve_one_multiline(
    conn: iroh::endpoint::Connection,
    source: &Source,
    show_progress: bool,
    label: String,
) -> Result<ServeOutcome> {
    serve_one_inner(conn, source, show_progress, Some(label)).await
}

async fn serve_one_inner(
    conn: iroh::endpoint::Connection,
    source: &Source,
    show_progress: bool,
    multiline_label: Option<String>,
) -> Result<ServeOutcome> {
    let (mut send, mut recv) =
        match accept_transfer_stream(&conn, multiline_label.as_deref(), show_progress).await {
            Ok(streams) => streams,
            Err(err) if err.to_string().contains("timed out") => return Ok(ServeOutcome::Ignored),
            Err(err) => return Err(err).context("accept stream"),
        };
    let req = read_transfer_request(&mut recv, multiline_label.as_deref(), show_progress).await?;
    let resume_from = if req.is_empty() {
        0
    } else {
        postcard::from_bytes::<ResumeRequest>(&req)
            .context("parse resume request")?
            .resume_from
    };
    match multiline_label {
        Some(label) => {
            source
                .stream_to_multiline(&mut send, resume_from, show_progress, label)
                .await?;
        }
        None => {
            source
                .stream_to(&mut send, resume_from, show_progress)
                .await?
        }
    }
    send.finish().context("finish payload")?;
    conn.closed().await;
    Ok(ServeOutcome::Sent)
}

async fn accept_transfer_stream(
    conn: &iroh::endpoint::Connection,
    multiline_label: Option<&str>,
    show_progress: bool,
) -> Result<(iroh::endpoint::SendStream, iroh::endpoint::RecvStream)> {
    let Some(label) = multiline_label.filter(|_| show_progress) else {
        return Ok(conn.accept_bi().await?);
    };
    eprintln!("{label}: connected; waiting for transfer request");
    let accept = conn.accept_bi();
    tokio::pin!(accept);
    let mut ticker = tokio::time::interval(Duration::from_secs(1));
    ticker.tick().await;
    loop {
        tokio::select! {
            streams = &mut accept => return Ok(streams?),
            _ = ticker.tick() => eprintln!("{label}: waiting for transfer request"),
        }
    }
}

async fn read_transfer_request(
    recv: &mut iroh::endpoint::RecvStream,
    multiline_label: Option<&str>,
    show_progress: bool,
) -> Result<Vec<u8>> {
    let read = recv.read_to_end(64);
    tokio::pin!(read);
    let Some(label) = multiline_label.filter(|_| show_progress) else {
        return read.await.context("read request");
    };
    let mut ticker = tokio::time::interval(Duration::from_secs(1));
    ticker.tick().await;
    loop {
        tokio::select! {
            request = &mut read => return request.context("read request"),
            _ = ticker.tick() => eprintln!("{label}: waiting for transfer request"),
        }
    }
}

pub(crate) fn filter_local_addrs(addr: iroh::EndpointAddr) -> iroh::EndpointAddr {
    let addrs = addr
        .addrs
        .into_iter()
        .filter(|a| a.is_ip())
        .collect::<std::collections::BTreeSet<_>>();
    iroh::EndpointAddr { id: addr.id, addrs }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        command::RecvArgs,
        ticket::{PayloadKind, Ticket},
        transport::source::md5_bytes,
    };
    use iroh::{EndpointAddr, SecretKey, TransportAddr};
    use std::net::{Ipv4Addr, SocketAddr};

    #[test]
    fn local_filter_drops_relays() {
        let addr = EndpointAddr::from_parts(
            SecretKey::generate().public(),
            [
                TransportAddr::Relay("https://example.com".parse().unwrap()),
                TransportAddr::Ip(SocketAddr::from((Ipv4Addr::LOCALHOST, 1234))),
            ],
        );
        let filtered = filter_local_addrs(addr);
        assert_eq!(filtered.relay_urls().count(), 0);
        assert_eq!(filtered.ip_addrs().count(), 1);
    }

    #[tokio::test]
    async fn file_plan_skips_identical_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("same.txt");
        std::fs::write(&path, b"same").unwrap();
        let ticket = test_ticket("same.txt", Some(4), Some(md5_bytes(b"same")));
        let plan = plan_file_receive(&test_recv_args(), &ticket, &path, &RecvTrace::new(false))
            .await
            .unwrap();
        assert_eq!(plan, FilePlan::Skip);
    }

    #[tokio::test]
    async fn file_plan_resumes_shorter_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("partial.txt");
        std::fs::write(&path, b"part").unwrap();
        let ticket = test_ticket("partial.txt", Some(10), Some(md5_bytes(b"partial-all")));
        let plan = plan_file_receive(&test_recv_args(), &ticket, &path, &RecvTrace::new(false))
            .await
            .unwrap();
        assert_eq!(plan, FilePlan::Download { resume_from: 4 });
    }

    #[tokio::test]
    async fn file_plan_overwrites_same_size_different_hash() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("changed.txt");
        std::fs::write(&path, b"old").unwrap();
        let ticket = test_ticket("changed.txt", Some(3), Some(md5_bytes(b"new")));
        let plan = plan_file_receive(&test_recv_args(), &ticket, &path, &RecvTrace::new(false))
            .await
            .unwrap();
        assert_eq!(plan, FilePlan::Download { resume_from: 0 });
    }

    fn test_ticket(name: &str, size: Option<u64>, content_md5: Option<[u8; 16]>) -> Ticket {
        Ticket::peer(
            EndpointAddr::from_parts(
                SecretKey::generate().public(),
                [TransportAddr::Ip(SocketAddr::from((
                    Ipv4Addr::LOCALHOST,
                    1234,
                )))],
            ),
            name.to_string(),
            PayloadKind::File,
            size,
            content_md5,
        )
    }

    fn test_recv_args() -> RecvArgs {
        RecvArgs {
            ticket: "ii1test".to_string(),
            out_dir: None,
            stdout: false,
            overwrite: false,
            resume: false,
            local: false,
            trace: false,
        }
    }
}
