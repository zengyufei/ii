use crate::{
    cli::{RecvArgs, SendArgs},
    ticket::{PayloadKind, ResumeRequest, Ticket},
};
use anyhow::{Context, Result, bail};
use iroh::{Endpoint, RelayMap, RelayMode, SecretKey, endpoint::presets};
use std::{
    ffi::OsStr,
    io::{IsTerminal, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use tempfile::NamedTempFile;
use tokio::{
    fs,
    io::{self, AsyncRead, AsyncReadExt, AsyncSeekExt, AsyncWrite, AsyncWriteExt},
};

const ALPN: &[u8] = b"ii/file/1";
const DEFAULT_CONNECT_FAST_PATH_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FilePlan {
    Download { resume_from: u64 },
    Skip,
}

struct RecvTrace {
    enabled: bool,
    started: Instant,
    last: Instant,
}

impl RecvTrace {
    fn new(enabled: bool) -> Self {
        let now = Instant::now();
        Self {
            enabled,
            started: now,
            last: now,
        }
    }

    fn info(&self, message: impl std::fmt::Display) {
        if self.enabled {
            eprintln!("ii recv trace: {message}");
        }
    }

    fn step(&mut self, label: &str) {
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

    fn finish(&self, bytes: u64) {
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

struct RecvProgress {
    enabled: bool,
    total: Option<u64>,
    completed: u64,
    transferred: u64,
    started: Instant,
    last_draw: Instant,
    last_rate_completed: u64,
}

impl RecvProgress {
    fn new(enabled: bool, total: Option<u64>, completed: u64) -> Self {
        let now = Instant::now();
        Self {
            enabled,
            total,
            completed,
            transferred: 0,
            started: now,
            last_draw: now,
            last_rate_completed: completed,
        }
    }

    fn advance(&mut self, bytes: u64) {
        self.completed = self.completed.saturating_add(bytes);
        self.transferred = self.transferred.saturating_add(bytes);
        if self.enabled && self.last_draw.elapsed() >= Duration::from_millis(250) {
            self.draw(false);
        }
    }

    fn finish(&mut self) {
        if self.enabled {
            self.draw(true);
            eprintln!();
        }
    }

    fn draw(&mut self, final_draw: bool) {
        let now = Instant::now();
        let elapsed = if final_draw {
            now.duration_since(self.started)
        } else {
            now.duration_since(self.last_draw)
        };
        let rate_bytes = if final_draw {
            self.transferred
        } else {
            self.completed.saturating_sub(self.last_rate_completed)
        };
        let bytes_per_second = if elapsed.as_secs_f64() > 0.0 {
            rate_bytes as f64 / elapsed.as_secs_f64()
        } else {
            0.0
        };

        let message = match self.total {
            Some(total) if total > 0 => {
                let pct = (self.completed.min(total) as f64 / total as f64) * 100.0;
                format!(
                    "ii recv: {} / {} ({:.1}%) | {}/s",
                    fmt_bytes(self.completed),
                    fmt_bytes(total),
                    pct,
                    fmt_bytes(bytes_per_second as u64)
                )
            }
            _ => format!(
                "ii recv: {} received | {}/s",
                fmt_bytes(self.completed),
                fmt_bytes(bytes_per_second as u64)
            ),
        };

        eprint!("\r{message:<96}");
        let _ = std::io::stderr().flush();
        self.last_draw = now;
        self.last_rate_completed = self.completed;
    }
}

fn should_show_progress(trace_enabled: bool) -> bool {
    std::io::stderr().is_terminal() && !trace_enabled
}

fn trace_endpoint_addr(label: &str, addr: &iroh::EndpointAddr, trace: &RecvTrace) {
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

fn payload_kind_name(kind: PayloadKind) -> &'static str {
    match kind {
        PayloadKind::File => "file",
        PayloadKind::Dir => "dir",
        PayloadKind::Stdin => "stdin",
    }
}

fn fmt_duration(duration: std::time::Duration) -> String {
    let ms = duration.as_millis();
    if ms < 1_000 {
        format!("{ms}ms")
    } else {
        format!("{:.2}s", duration.as_secs_f64())
    }
}

fn fmt_bytes(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    let bytes_f = bytes as f64;
    if bytes_f >= GIB {
        format!("{:.2} GiB", bytes_f / GIB)
    } else if bytes_f >= MIB {
        format!("{:.2} MiB", bytes_f / MIB)
    } else if bytes_f >= KIB {
        format!("{:.2} KiB", bytes_f / KIB)
    } else {
        format!("{bytes} B")
    }
}

async fn md5_path(path: PathBuf) -> Result<[u8; 16]> {
    tokio::task::spawn_blocking(move || md5_path_blocking(&path))
        .await
        .context("hash task")?
}

fn md5_path_blocking(path: &Path) -> Result<[u8; 16]> {
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("open file for md5 {}", path.display()))?;
    let mut ctx = md5::Context::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .with_context(|| format!("read file for md5 {}", path.display()))?;
        if n == 0 {
            break;
        }
        ctx.consume(&buf[..n]);
    }
    Ok(ctx.compute().0)
}

pub async fn send(args: SendArgs) -> Result<()> {
    let source = Source::open(args.path.clone(), args.name.clone()).await?;
    let endpoint = bind_endpoint(relay_mode_for_send(&args)?).await?;

    if should_wait_online(&args) {
        endpoint.online().await;
    }

    let ticket = Ticket {
        version: 2,
        endpoint: endpoint.addr(),
        name: source.name().to_string(),
        kind: source.kind(),
        size: source.size(),
        content_md5: source.content_md5(),
    };
    let ticket_str = ticket.encode()?;
    print_ticket(&ticket_str, args.copy, args.output.clone())?;

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
                match serve_one(conn, &source).await {
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

pub async fn recv(args: RecvArgs) -> Result<()> {
    let mut trace = RecvTrace::new(args.trace);
    let show_progress = should_show_progress(args.trace);
    trace.info(format_args!(
        "mode: {}",
        if args.local {
            "local-only"
        } else {
            "default relay path"
        }
    ));

    let ticket = Ticket::decode(&args.ticket)?;
    trace.step("decode ticket");
    trace.info(format_args!(
        "ticket: kind={}, name={}, size={}",
        payload_kind_name(ticket.kind),
        ticket.name.as_str(),
        ticket
            .size
            .map(|size| size.to_string())
            .unwrap_or_else(|| "unknown".to_string())
    ));
    trace_endpoint_addr("ticket endpoints", &ticket.endpoint, &trace);

    let out_dir = args
        .out_dir
        .clone()
        .unwrap_or(std::env::current_dir().context("current dir")?);
    let file_target =
        if matches!(ticket.kind, PayloadKind::File | PayloadKind::Stdin) && !args.stdout {
            let path = out_dir.join(&ticket.name);
            let plan = plan_file_receive(&args, &ticket, &path, &trace).await?;
            if plan == FilePlan::Skip {
                trace.info(format_args!("skipped identical file {}", path.display()));
                eprintln!("ii recv: skipped identical file {}", path.display());
                return Ok(());
            }
            Some((path, plan))
        } else {
            None
        };

    let endpoint = bind_endpoint(if args.local {
        RelayMode::Disabled
    } else {
        RelayMode::Default
    })
    .await?;
    trace.step("bind endpoint");
    if !args.local {
        trace.info("waiting for endpoint to go online");
        endpoint.online().await;
        trace.step("wait online");
    }

    let mut endpoint_addr = ticket.endpoint.clone();
    if args.local {
        endpoint_addr = filter_local_addrs(endpoint_addr);
        trace_endpoint_addr("local-filtered endpoints", &endpoint_addr, &trace);
    }
    if endpoint_addr.addrs.is_empty() {
        bail!("ticket has no usable addresses for this mode");
    }

    let conn = connect_to_sender(&endpoint, endpoint_addr, args.local, &trace).await?;
    trace.step("connect to sender");

    let (mut send, recv) = conn.open_bi().await.context("open transfer stream")?;
    trace.step("open transfer stream");

    let resume_from = file_target
        .as_ref()
        .map(|(_, plan)| match plan {
            FilePlan::Download { resume_from } => *resume_from,
            FilePlan::Skip => 0,
        })
        .unwrap_or(0);
    if resume_from > 0 {
        trace.info(format_args!("resume from byte {}", resume_from));
    }
    let request = ResumeRequest { resume_from };
    let request_bytes = postcard::to_stdvec(&request).context("encode resume request")?;
    send.write_all(&request_bytes)
        .await
        .context("send request")?;
    send.finish().context("finish request")?;
    trace.step("send transfer request");

    let bytes_written = match ticket.kind {
        PayloadKind::File | PayloadKind::Stdin => {
            if args.stdout {
                copy_to_stdout(recv, ticket.size, show_progress).await?
            } else {
                let (path, plan) = file_target.expect("file target exists");
                let resume_from = match plan {
                    FilePlan::Download { resume_from } => resume_from,
                    FilePlan::Skip => 0,
                };
                write_to_file(recv, path, resume_from, ticket.size, show_progress).await?
            }
        }
        PayloadKind::Dir => {
            if args.stdout {
                bail!("--stdout is not supported for directory tickets");
            }
            extract_tar_stream(recv, out_dir, ticket.size, show_progress).await?
        }
    };
    trace.step("receive payload");
    trace.info(format_args!("received {} bytes", bytes_written));

    conn.close(0u32.into(), b"done");
    endpoint.close().await;
    trace.finish(bytes_written);
    Ok(())
}

async fn bind_endpoint(relay_mode: RelayMode) -> Result<Endpoint> {
    let secret_key = SecretKey::generate();
    let endpoint = Endpoint::builder(presets::N0)
        .secret_key(secret_key)
        .alpns(vec![ALPN.to_vec()])
        .relay_mode(relay_mode)
        .bind()
        .await
        .context("bind endpoint")?;
    Ok(endpoint)
}

async fn connect_to_sender(
    endpoint: &Endpoint,
    endpoint_addr: iroh::EndpointAddr,
    local_only: bool,
    trace: &RecvTrace,
) -> Result<iroh::endpoint::Connection> {
    if local_only {
        trace.info("connecting to sender");
        return endpoint
            .connect(endpoint_addr, ALPN)
            .await
            .context("connect to sender");
    }

    let relay_only = relay_only_addr(&endpoint_addr);
    if relay_only.is_none() {
        trace.info("connecting to sender");
        return endpoint
            .connect(endpoint_addr, ALPN)
            .await
            .context("connect to sender");
    }

    trace.info(format_args!(
        "connecting to sender, full address set gets {} before relay-only fallback",
        fmt_duration(DEFAULT_CONNECT_FAST_PATH_TIMEOUT)
    ));
    match tokio::time::timeout(
        DEFAULT_CONNECT_FAST_PATH_TIMEOUT,
        endpoint.connect(endpoint_addr, ALPN),
    )
    .await
    {
        Ok(result) => result.context("connect to sender"),
        Err(_) => {
            let relay_only = relay_only.expect("checked above");
            trace.info("full address connect timed out; retrying relay-only");
            trace_endpoint_addr("relay-only endpoints", &relay_only, trace);
            endpoint
                .connect(relay_only, ALPN)
                .await
                .context("connect to sender via relay")
        }
    }
}

fn relay_only_addr(addr: &iroh::EndpointAddr) -> Option<iroh::EndpointAddr> {
    let addrs = addr
        .addrs
        .iter()
        .filter(|addr| addr.is_relay())
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    (!addrs.is_empty()).then(|| iroh::EndpointAddr { id: addr.id, addrs })
}

async fn plan_file_receive(
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
        if !matches!(ticket.kind, PayloadKind::File | PayloadKind::Stdin) {
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
    let ticket_size = ticket.size;
    if let Some(expected_hash) = ticket.content_md5 {
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

fn relay_mode_for_send(args: &SendArgs) -> Result<RelayMode> {
    if args.local || args.no_relay {
        return Ok(RelayMode::Disabled);
    }
    if let Some(url) = &args.relay {
        return Ok(RelayMode::Custom(RelayMap::from(url.clone())));
    }
    Ok(RelayMode::Default)
}

fn should_wait_online(args: &SendArgs) -> bool {
    !args.local && !args.no_relay
}

enum ServeOutcome {
    Sent,
    Ignored,
}

async fn serve_one(conn: iroh::endpoint::Connection, source: &Source) -> Result<ServeOutcome> {
    let (mut send, mut recv) = match conn.accept_bi().await {
        Ok(streams) => streams,
        Err(err) if err.to_string().contains("timed out") => return Ok(ServeOutcome::Ignored),
        Err(err) => return Err(err).context("accept stream"),
    };
    let req = recv.read_to_end(64).await.context("read request")?;
    let resume_from = if req.is_empty() {
        0
    } else {
        postcard::from_bytes::<ResumeRequest>(&req)
            .context("parse resume request")?
            .resume_from
    };
    source.stream_to(&mut send, resume_from).await?;
    send.finish().context("finish payload")?;
    conn.closed().await;
    Ok(ServeOutcome::Sent)
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

enum Backing {
    Path(PathBuf),
    Temp(NamedTempFile),
}

struct Source {
    backing: Backing,
    name: String,
    kind: PayloadKind,
    size: u64,
    content_md5: Option<[u8; 16]>,
}

impl Source {
    async fn open(path: Option<PathBuf>, override_name: Option<String>) -> Result<Self> {
        match path {
            None => Self::from_stdin(override_name).await,
            Some(path) if path.is_dir() => Self::from_dir(path, override_name).await,
            Some(path) => Self::from_file(path, override_name).await,
        }
    }

    async fn from_stdin(override_name: Option<String>) -> Result<Self> {
        if std::io::stdin().is_terminal() {
            bail!("no path provided and stdin is interactive");
        }
        let name = override_name.unwrap_or_else(|| "stdin".to_string());
        let temp = NamedTempFile::new().context("create temp file")?;
        let path = temp.path().to_path_buf();
        let mut file = fs::File::from_std(temp.reopen().context("reopen temp file")?);
        let mut stdin = tokio::io::stdin();
        io::copy(&mut stdin, &mut file)
            .await
            .context("read stdin")?;
        file.flush().await.context("flush stdin temp file")?;
        let size = fs::metadata(&path)
            .await
            .context("stat stdin temp file")?
            .len();
        let content_md5 = md5_path(path).await?;
        Ok(Self {
            backing: Backing::Temp(temp),
            name,
            kind: PayloadKind::Stdin,
            size,
            content_md5: Some(content_md5),
        })
    }

    async fn from_file(path: PathBuf, override_name: Option<String>) -> Result<Self> {
        let meta = fs::metadata(&path).await.context("stat source file")?;
        let name = override_name.unwrap_or_else(|| {
            path.file_name()
                .and_then(OsStr::to_str)
                .unwrap_or("ii-file")
                .to_string()
        });
        let content_md5 = md5_path(path.clone()).await?;
        Ok(Self {
            backing: Backing::Path(path),
            name,
            kind: PayloadKind::File,
            size: meta.len(),
            content_md5: Some(content_md5),
        })
    }

    async fn from_dir(path: PathBuf, override_name: Option<String>) -> Result<Self> {
        let name = override_name.unwrap_or_else(|| {
            path.file_name()
                .and_then(OsStr::to_str)
                .unwrap_or("ii-dir")
                .to_string()
        });
        let temp = NamedTempFile::new().context("create temp archive")?;
        let archive_path = temp.path().to_path_buf();
        let src_path = path.clone();
        let archive_name = name.clone();
        let archive_path_for_task = archive_path.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let file = std::fs::File::create(&archive_path_for_task).context("create archive")?;
            let mut builder = tar::Builder::new(file);
            builder
                .append_dir_all(&archive_name, &src_path)
                .context("build tar archive")?;
            builder.finish().context("finish tar archive")?;
            Ok(())
        })
        .await
        .context("archive task")??;
        let size = std::fs::metadata(&archive_path)
            .context("stat tar archive")?
            .len();
        Ok(Self {
            backing: Backing::Temp(temp),
            name,
            kind: PayloadKind::Dir,
            size,
            content_md5: None,
        })
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn kind(&self) -> PayloadKind {
        self.kind
    }

    fn size(&self) -> Option<u64> {
        Some(self.size)
    }

    fn content_md5(&self) -> Option<[u8; 16]> {
        self.content_md5
    }

    async fn stream_to<W: AsyncWrite + Unpin>(&self, out: &mut W, resume_from: u64) -> Result<()> {
        if resume_from > 0 && self.kind == PayloadKind::Dir {
            bail!("resume is only supported for regular files");
        }
        let mut file = self.open_file().await?;
        if resume_from > 0 {
            file.seek(std::io::SeekFrom::Start(resume_from))
                .await
                .context("seek resume offset")?;
        }
        io::copy(&mut file, out).await.context("stream payload")?;
        Ok(())
    }

    async fn open_file(&self) -> Result<fs::File> {
        match &self.backing {
            Backing::Path(path) => fs::File::open(path).await.context("open source file"),
            Backing::Temp(temp) => fs::File::open(temp.path())
                .await
                .context("open temp source"),
        }
    }
}

fn filter_local_addrs(addr: iroh::EndpointAddr) -> iroh::EndpointAddr {
    let addrs = addr
        .addrs
        .into_iter()
        .filter(|a| a.is_ip())
        .collect::<std::collections::BTreeSet<_>>();
    iroh::EndpointAddr { id: addr.id, addrs }
}

async fn write_to_file(
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
    let mut progress = RecvProgress::new(show_progress, total_size, resume_from);
    let bytes = copy_with_progress(&mut recv, &mut file, &mut progress)
        .await
        .with_context(|| format!("write destination {}", path.display()))?;
    progress.finish();
    file.flush()
        .await
        .with_context(|| format!("flush destination {}", path.display()))?;
    Ok(bytes)
}

async fn copy_to_stdout(
    mut recv: iroh::endpoint::RecvStream,
    total_size: Option<u64>,
    show_progress: bool,
) -> Result<u64> {
    let mut stdout = io::stdout();
    let mut progress = RecvProgress::new(show_progress, total_size, 0);
    let bytes = copy_with_progress(&mut recv, &mut stdout, &mut progress)
        .await
        .context("write stdout")?;
    progress.finish();
    stdout.flush().await.ok();
    Ok(bytes)
}

async fn extract_tar_stream(
    mut recv: iroh::endpoint::RecvStream,
    path: PathBuf,
    total_size: Option<u64>,
    show_progress: bool,
) -> Result<u64> {
    fs::create_dir_all(&path)
        .await
        .with_context(|| format!("create output dir {}", path.display()))?;
    let temp = NamedTempFile::new().context("create temp tar")?;
    let temp_path = temp.path().to_path_buf();
    let mut file = fs::File::from_std(temp.reopen().context("reopen temp tar")?);
    let mut progress = RecvProgress::new(show_progress, total_size, 0);
    let bytes = copy_with_progress(&mut recv, &mut file, &mut progress)
        .await
        .context("buffer tar")?;
    progress.finish();
    file.flush().await.context("flush tar")?;
    let extract_path = path.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let file = std::fs::File::open(&temp_path).context("open tar")?;
        let mut archive = tar::Archive::new(file);
        archive.unpack(&extract_path).context("unpack tar")?;
        Ok(())
    })
    .await
    .context("extract task")??;
    Ok(bytes)
}

async fn copy_with_progress<R, W>(
    reader: &mut R,
    writer: &mut W,
    progress: &mut RecvProgress,
) -> Result<u64>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut buf = [0u8; 64 * 1024];
    let mut written = 0u64;
    loop {
        let n = reader.read(&mut buf).await.context("read payload")?;
        if n == 0 {
            break;
        }
        writer.write_all(&buf[..n]).await.context("write payload")?;
        let n = n as u64;
        written = written.saturating_add(n);
        progress.advance(n);
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use iroh::{EndpointAddr, TransportAddr};
    use std::net::{Ipv4Addr, SocketAddr};

    #[test]
    fn ticket_round_trip() {
        let ticket = Ticket {
            version: 2,
            endpoint: EndpointAddr::from_parts(
                SecretKey::generate().public(),
                [TransportAddr::Ip(SocketAddr::from((
                    Ipv4Addr::LOCALHOST,
                    1234,
                )))],
            ),
            name: "hello.txt".into(),
            kind: PayloadKind::File,
            size: Some(12),
            content_md5: Some([1; 16]),
        };
        let raw = ticket.encode().unwrap();
        let decoded = Ticket::decode(&raw).unwrap();
        assert_eq!(ticket, decoded);
    }

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
        let ticket = test_ticket("same.txt", Some(4), Some(md5::compute(b"same").0));
        let args = test_recv_args();
        let trace = RecvTrace::new(false);
        let plan = plan_file_receive(&args, &ticket, &path, &trace)
            .await
            .unwrap();
        assert_eq!(plan, FilePlan::Skip);
    }

    #[tokio::test]
    async fn file_plan_resumes_shorter_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("partial.txt");
        std::fs::write(&path, b"part").unwrap();
        let ticket = test_ticket(
            "partial.txt",
            Some(10),
            Some(md5::compute(b"partial-all").0),
        );
        let args = test_recv_args();
        let trace = RecvTrace::new(false);
        let plan = plan_file_receive(&args, &ticket, &path, &trace)
            .await
            .unwrap();
        assert_eq!(plan, FilePlan::Download { resume_from: 4 });
    }

    #[tokio::test]
    async fn file_plan_overwrites_same_size_different_hash() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("changed.txt");
        std::fs::write(&path, b"old").unwrap();
        let ticket = test_ticket("changed.txt", Some(3), Some(md5::compute(b"new").0));
        let args = test_recv_args();
        let trace = RecvTrace::new(false);
        let plan = plan_file_receive(&args, &ticket, &path, &trace)
            .await
            .unwrap();
        assert_eq!(plan, FilePlan::Download { resume_from: 0 });
    }

    fn test_ticket(name: &str, size: Option<u64>, content_md5: Option<[u8; 16]>) -> Ticket {
        Ticket {
            version: 2,
            endpoint: EndpointAddr::from_parts(
                SecretKey::generate().public(),
                [TransportAddr::Ip(SocketAddr::from((
                    Ipv4Addr::LOCALHOST,
                    1234,
                )))],
            ),
            name: name.to_string(),
            kind: PayloadKind::File,
            size,
            content_md5,
        }
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
