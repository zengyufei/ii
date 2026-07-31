use crate::{
    cli::{RecvArgs, SendArgs, WebArgs, WebrtcArgs},
    storage,
    ticket::{
        FtpPortableCredentials, PayloadKind, ResumeRequest, SftpPortableAuth,
        SftpPortableCredentials, Ticket, WebDavPortableCredentials,
    },
};
use anyhow::{Context, Result, bail};
use futures_util::TryStreamExt;
use iroh::{Endpoint, RelayMap, RelayMode, SecretKey, TransportAddr, endpoint::presets};
use iroh_relay::tls::CaTlsConfig;
use percent_encoding::{NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};
use qrcodegen::{QrCode, QrCodeEcc};
use russh::{
    client::{self as ssh_client, Handler as SshClientHandler},
    keys::{HashAlg, PrivateKeyWithHashAlg, decode_secret_key},
};
use russh_sftp::{client::SftpSession, protocol::OpenFlags};
use rustls::{
    DigitallySignedStruct, SignatureScheme,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    crypto::CryptoProvider,
    pki_types::{CertificateDer, ServerName, UnixTime},
};
use std::{
    collections::{BTreeMap, VecDeque},
    ffi::OsStr,
    io::{IsTerminal, Read, Write},
    net::{Ipv4Addr, SocketAddr, UdpSocket},
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use suppaftp::tokio::AsyncFtpStream;
use tempfile::NamedTempFile;
use time::{OffsetDateTime, format_description::FormatItem, macros::format_description};
use tokio::{
    fs,
    io::{self, AsyncRead, AsyncReadExt, AsyncSeekExt, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tokio_util::io::ReaderStream;

const ALPN: &[u8] = b"ii/file/1";
const DEFAULT_CONNECT_FAST_PATH_TIMEOUT: Duration = Duration::from_secs(3);
const WEB_DIRECTORY_TIME_FORMAT: &[FormatItem<'static>] =
    format_description!("[year]-[month]-[day] [hour]:[minute]");
static NEXT_OBJECT_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransferEvent {
    Started,
    TicketReady(String),
    Completed,
    Failed(String),
}

#[derive(Debug, Clone)]
enum EndpointPolicy {
    Standard(RelayMode),
    SelfSignedRelayOnly(iroh::RelayUrl),
    TrustedRelayOnly(iroh::RelayUrl),
}

impl EndpointPolicy {
    fn standard(relay_mode: RelayMode) -> Self {
        Self::Standard(relay_mode)
    }

    fn relay_mode(&self) -> RelayMode {
        match self {
            Self::Standard(mode) => mode.clone(),
            Self::SelfSignedRelayOnly(url) | Self::TrustedRelayOnly(url) => {
                RelayMode::Custom(RelayMap::from(url.clone()))
            }
        }
    }

    fn is_relay_only(&self) -> bool {
        matches!(
            self,
            Self::SelfSignedRelayOnly(_) | Self::TrustedRelayOnly(_)
        )
    }

    fn accepts_self_signed_relay(&self) -> bool {
        matches!(self, Self::SelfSignedRelayOnly(_))
    }
}

#[derive(Debug)]
struct AcceptAnyRelayCertificate {
    crypto_provider: Arc<CryptoProvider>,
}

impl ServerCertVerifier for AcceptAnyRelayCertificate {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer,
        _intermediates: &[CertificateDer],
        _server_name: &ServerName,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.crypto_provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

fn accept_self_signed_relay_tls() -> CaTlsConfig {
    CaTlsConfig::custom_server_cert_verifier(Arc::new(|crypto_provider| {
        Ok(Arc::new(AcceptAnyRelayCertificate { crypto_provider }))
    }))
}

fn unique_object_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or_default();
    let sequence = NEXT_OBJECT_ID.fetch_add(1, Ordering::Relaxed) as u32;
    format!("{nanos:016x}{:08x}{sequence:08x}", std::process::id())
}

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

struct TransferProgress {
    label: &'static str,
    enabled: bool,
    total: Option<u64>,
    completed: u64,
    transferred: u64,
    started: Instant,
    last_draw: Instant,
    last_rate_completed: u64,
}

impl TransferProgress {
    fn new(label: &'static str, enabled: bool, total: Option<u64>, completed: u64) -> Self {
        let now = Instant::now();
        Self {
            label,
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

        let message = if final_draw {
            format!(
                "{}: done: {} in {} | avg {}/s",
                self.label,
                fmt_bytes(self.completed),
                fmt_duration(now.duration_since(self.started)),
                fmt_bytes(bytes_per_second as u64)
            )
        } else {
            match self.total {
                Some(total) if total > 0 => {
                    let pct = (self.completed.min(total) as f64 / total as f64) * 100.0;
                    format!(
                        "{}: {} / {} ({:.1}%) | {}/s",
                        self.label,
                        fmt_bytes(self.completed),
                        fmt_bytes(total),
                        pct,
                        fmt_bytes(bytes_per_second as u64)
                    )
                }
                _ => format!(
                    "{}: {} received | {}/s",
                    self.label,
                    fmt_bytes(self.completed),
                    fmt_bytes(bytes_per_second as u64)
                ),
            }
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
    let mut ctx = <md5::Md5 as md5::Digest>::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .with_context(|| format!("read file for md5 {}", path.display()))?;
        if n == 0 {
            break;
        }
        md5::Digest::update(&mut ctx, &buf[..n]);
    }
    Ok(finalize_md5(ctx))
}

#[cfg(test)]
fn md5_bytes(bytes: &[u8]) -> [u8; 16] {
    let mut ctx = <md5::Md5 as md5::Digest>::new();
    md5::Digest::update(&mut ctx, bytes);
    finalize_md5(ctx)
}

fn finalize_md5(ctx: md5::Md5) -> [u8; 16] {
    let digest = md5::Digest::finalize(ctx);
    let mut out = [0u8; 16];
    out.copy_from_slice(&digest);
    out
}

pub async fn send(args: SendArgs) -> Result<()> {
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

enum WebContent {
    Download {
        source: Source,
        download_name: String,
        download_qr_svg: String,
    },
    Directory {
        root: PathBuf,
    },
}

struct WebShare {
    content: WebContent,
    upload_dir: PathBuf,
    web_token: Option<String>,
}

struct WebRequest {
    method: String,
    target: String,
    content_length: Option<u64>,
    body: Vec<u8>,
}

struct WebDirectoryEntry {
    name: String,
    is_dir: bool,
    modified: String,
    size: String,
}

struct LanWebServer {
    listener: TcpListener,
    url: String,
}

struct WebRtcServer {
    state: Mutex<WebRtcState>,
}

struct WebRtcState {
    next_peer_id: u64,
    peers: BTreeMap<u64, WebRtcPeer>,
}

struct WebRtcPeer {
    last_seen: Instant,
    signals: VecDeque<WebRtcSignal>,
}

struct WebRtcSignal {
    from: u64,
    body: Vec<u8>,
}

#[derive(Debug)]
enum WebRtcRelayError {
    MissingPeer,
    QueueFull,
}

const WEBRTC_PEER_TTL: Duration = Duration::from_secs(30);
const WEBRTC_MAX_SIGNAL_BYTES: u64 = 128 * 1024;
const WEBRTC_MAX_PENDING_SIGNALS: usize = 64;

async fn send_web(args: SendArgs) -> Result<()> {
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
        args.web_token,
    )
    .await
}

pub async fn web(args: WebArgs) -> Result<()> {
    let start_dir = std::env::current_dir().context("read current directory for web service")?;
    let root = web_directory_root(&start_dir, args.dir.as_deref()).await?;
    let upload_dir = web_upload_dir(&start_dir, args.web_upload_dir.as_deref());
    serve_web(WebContent::Directory { root }, upload_dir, args.web_token).await
}

pub async fn webrtc(args: WebrtcArgs) -> Result<()> {
    let server = Arc::new(WebRtcServer::new());
    let lan = start_lan_web_server(args.web_token.as_deref(), "ii webrtc").await?;

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

async fn web_directory_root(start_dir: &Path, requested_dir: Option<&Path>) -> Result<PathBuf> {
    let path = match requested_dir {
        Some(path) if path.is_absolute() => path.to_path_buf(),
        Some(path) => start_dir.join(path),
        None => start_dir.to_path_buf(),
    };
    let root = fs::canonicalize(&path)
        .await
        .with_context(|| format!("read web directory {}", path.display()))?;
    let metadata = fs::metadata(&root)
        .await
        .with_context(|| format!("read web directory {}", root.display()))?;
    if !metadata.is_dir() {
        bail!("web path is not a directory: {}", path.display());
    }
    Ok(root)
}

async fn serve_web(
    mut content: WebContent,
    upload_dir: PathBuf,
    web_token: Option<String>,
) -> Result<()> {
    let lan = start_lan_web_server(web_token.as_deref(), "ii web").await?;
    if let WebContent::Download {
        download_qr_svg, ..
    } = &mut content
    {
        *download_qr_svg = web_qr_svg(&format!("{}download", lan.url))?;
    }
    let share = Arc::new(WebShare {
        content,
        upload_dir,
        web_token,
    });

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            accepted = lan.listener.accept() => {
                let (stream, _) = accepted.context("accept web connection")?;
                let share = Arc::clone(&share);
                tokio::spawn(async move {
                    if let Err(err) = serve_web_connection(stream, share).await {
                        eprintln!("ii web: request failed: {err:#}");
                    }
                });
            }
        }
    }
    Ok(())
}

async fn start_lan_web_server(web_token: Option<&str>, label: &str) -> Result<LanWebServer> {
    let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0))
        .await
        .with_context(|| format!("bind {label} server"))?;
    let port = listener
        .local_addr()
        .with_context(|| format!("read {label} server address"))?
        .port();
    let host = local_web_host();
    let root_path = web_root_path(web_token);
    let url = format!("http://{host}:{port}{root_path}");

    if std::io::stdout().is_terminal() {
        print!("{}", web_qr_terminal(&url)?);
    }
    println!("{label}: {url}");
    println!();
    println!("other:");
    for host in web_other_hosts(host, web_interface_ipv4_addrs()) {
        println!("http://{host}:{port}{root_path}");
    }
    println!();
    println!("press Ctrl+C to stop sharing");

    Ok(LanWebServer { listener, url })
}

impl WebRtcServer {
    fn new() -> Self {
        Self {
            state: Mutex::new(WebRtcState {
                next_peer_id: 1,
                peers: BTreeMap::new(),
            }),
        }
    }

    fn join(&self) -> Option<u64> {
        let now = Instant::now();
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        prune_webrtc_peers(&mut state, now);
        let peer_id = state.next_peer_id;
        state.next_peer_id = state.next_peer_id.checked_add(1)?;
        state.peers.insert(
            peer_id,
            WebRtcPeer {
                last_seen: now,
                signals: VecDeque::new(),
            },
        );
        Some(peer_id)
    }

    fn peers(&self, peer_id: u64) -> Option<Vec<u64>> {
        let now = Instant::now();
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        prune_webrtc_peers(&mut state, now);
        state.peers.get_mut(&peer_id)?.last_seen = now;
        Some(
            state
                .peers
                .keys()
                .copied()
                .filter(|id| *id != peer_id)
                .collect(),
        )
    }

    fn heartbeat(&self, peer_id: u64) -> bool {
        let now = Instant::now();
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        prune_webrtc_peers(&mut state, now);
        let Some(peer) = state.peers.get_mut(&peer_id) else {
            return false;
        };
        peer.last_seen = now;
        true
    }

    fn relay(&self, from: u64, to: u64, body: Vec<u8>) -> Result<(), WebRtcRelayError> {
        let now = Instant::now();
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        prune_webrtc_peers(&mut state, now);
        let Some(sender) = state.peers.get_mut(&from) else {
            return Err(WebRtcRelayError::MissingPeer);
        };
        sender.last_seen = now;
        let Some(recipient) = state.peers.get_mut(&to) else {
            return Err(WebRtcRelayError::MissingPeer);
        };
        if recipient.signals.len() >= WEBRTC_MAX_PENDING_SIGNALS {
            return Err(WebRtcRelayError::QueueFull);
        }
        recipient.signals.push_back(WebRtcSignal { from, body });
        Ok(())
    }

    fn poll(&self, peer_id: u64) -> Option<Option<WebRtcSignal>> {
        let now = Instant::now();
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        prune_webrtc_peers(&mut state, now);
        let peer = state.peers.get_mut(&peer_id)?;
        peer.last_seen = now;
        Some(peer.signals.pop_front())
    }
}

fn prune_webrtc_peers(state: &mut WebRtcState, now: Instant) {
    state
        .peers
        .retain(|_, peer| now.duration_since(peer.last_seen) < WEBRTC_PEER_TTL);
}

fn web_upload_dir(start_dir: &Path, configured_dir: Option<&Path>) -> PathBuf {
    match configured_dir {
        Some(dir) if dir.is_absolute() => dir.to_path_buf(),
        Some(dir) => start_dir.join(dir),
        None => start_dir.join("ii"),
    }
}

fn web_root_path(token: Option<&str>) -> String {
    match token {
        Some(token) => format!("/{token}/"),
        None => "/".to_string(),
    }
}

fn local_web_host() -> Ipv4Addr {
    let socket = match UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)) {
        Ok(socket) => socket,
        Err(_) => return Ipv4Addr::LOCALHOST,
    };
    if socket.connect((Ipv4Addr::new(8, 8, 8, 8), 80)).is_ok() {
        if let Ok(SocketAddr::V4(addr)) = socket.local_addr() {
            if !addr.ip().is_unspecified() {
                return *addr.ip();
            }
        }
    }
    Ipv4Addr::LOCALHOST
}

fn web_other_hosts(primary: Ipv4Addr, mut hosts: Vec<Ipv4Addr>) -> Vec<Ipv4Addr> {
    hosts.retain(|host| !host.is_loopback() && !host.is_unspecified() && *host != primary);
    hosts.sort_unstable();
    hosts.dedup();
    hosts
}

#[cfg(windows)]
fn web_interface_ipv4_addrs() -> Vec<Ipv4Addr> {
    use std::{
        ffi::c_void,
        mem,
        ptr::{self, NonNull},
    };

    const AF_INET: u32 = 2;
    const ERROR_BUFFER_OVERFLOW: u32 = 111;

    #[repr(C)]
    union AdapterHeader {
        alignment: u64,
        fields: AdapterHeaderFields,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct AdapterHeaderFields {
        length: u32,
        interface_index: u32,
    }

    #[repr(C)]
    struct AdapterAddresses {
        header: AdapterHeader,
        next: *mut AdapterAddresses,
        adapter_name: *mut i8,
        first_unicast_address: *mut UnicastAddress,
    }

    #[repr(C)]
    struct UnicastAddress {
        header: AdapterHeader,
        next: *mut UnicastAddress,
        address: SocketAddress,
    }

    #[repr(C)]
    struct SocketAddress {
        address: *const c_void,
        length: i32,
    }

    #[repr(C)]
    struct SockAddrIn {
        family: u16,
        port: u16,
        address: [u8; 4],
        zero: [u8; 8],
    }

    #[link(name = "iphlpapi")]
    unsafe extern "system" {
        fn GetAdaptersAddresses(
            family: u32,
            flags: u32,
            reserved: *mut c_void,
            addresses: *mut AdapterAddresses,
            size: *mut u32,
        ) -> u32;
    }

    let mut size = 15 * 1024;
    for _ in 0..2 {
        let words = (size as usize).div_ceil(mem::size_of::<usize>());
        let mut buffer = vec![0usize; words];
        let result = unsafe {
            GetAdaptersAddresses(
                AF_INET,
                0,
                ptr::null_mut(),
                buffer.as_mut_ptr().cast(),
                &mut size,
            )
        };
        if result == ERROR_BUFFER_OVERFLOW {
            continue;
        }
        if result != 0 {
            return Vec::new();
        }

        let mut hosts = Vec::new();
        let mut adapter = NonNull::new(buffer.as_mut_ptr().cast::<AdapterAddresses>());
        while let Some(current) = adapter {
            let mut unicast = unsafe { NonNull::new(current.as_ref().first_unicast_address) };
            while let Some(current) = unicast {
                let address = unsafe { current.as_ref().address.address };
                if !address.is_null() {
                    let address = unsafe { &*address.cast::<SockAddrIn>() };
                    if address.family == AF_INET as u16 {
                        hosts.push(Ipv4Addr::from(address.address));
                    }
                }
                unicast = unsafe { NonNull::new(current.as_ref().next) };
            }
            adapter = unsafe { NonNull::new(current.as_ref().next) };
        }
        return hosts;
    }
    Vec::new()
}

#[cfg(unix)]
fn web_interface_ipv4_addrs() -> Vec<Ipv4Addr> {
    use std::{
        ffi::{c_char, c_int, c_void},
        ptr,
    };

    #[repr(C)]
    struct IfAddrs {
        next: *mut IfAddrs,
        name: *mut c_char,
        flags: u32,
        address: *mut c_void,
        netmask: *mut c_void,
        destination: *mut c_void,
        data: *mut c_void,
    }

    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd"
    ))]
    #[repr(C)]
    struct SockAddrIn {
        length: u8,
        family: u8,
        port: u16,
        address: [u8; 4],
        zero: [u8; 8],
    }

    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd"
    )))]
    #[repr(C)]
    struct SockAddrIn {
        family: u16,
        port: u16,
        address: [u8; 4],
        zero: [u8; 8],
    }

    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd"
    ))]
    const AF_INET: u8 = 2;
    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd"
    )))]
    const AF_INET: u16 = 2;

    #[link(name = "c")]
    unsafe extern "C" {
        fn getifaddrs(addresses: *mut *mut IfAddrs) -> c_int;
        fn freeifaddrs(addresses: *mut IfAddrs);
    }

    let mut first = ptr::null_mut();
    if unsafe { getifaddrs(&mut first) } != 0 {
        return Vec::new();
    }

    let mut hosts = Vec::new();
    let mut current = first;
    while !current.is_null() {
        let address = unsafe { (*current).address };
        if !address.is_null() {
            let address = unsafe { &*address.cast::<SockAddrIn>() };
            if address.family == AF_INET {
                hosts.push(Ipv4Addr::from(address.address));
            }
        }
        current = unsafe { (*current).next };
    }
    unsafe { freeifaddrs(first) };
    hosts
}

#[cfg(not(any(windows, unix)))]
fn web_interface_ipv4_addrs() -> Vec<Ipv4Addr> {
    Vec::new()
}

async fn serve_web_connection(mut stream: TcpStream, share: Arc<WebShare>) -> Result<()> {
    let request = match read_web_request(&mut stream).await {
        Ok(request) => request,
        Err(err) => {
            let message = format!("bad request: {err}");
            return write_web_error(&mut stream, "400 Bad Request", &message).await;
        }
    };

    let Some(path) = web_request_path(&share, &request.target) else {
        return write_web_error(&mut stream, "404 Not Found", "not found").await;
    };

    match request.method.as_str() {
        "GET" => match &share.content {
            WebContent::Download {
                source,
                download_name,
                download_qr_svg,
            } => match path {
                "" => write_web_page(&mut stream, source, download_name, download_qr_svg).await,
                "download" => write_web_download(&mut stream, source, download_name).await,
                _ => write_web_error(&mut stream, "404 Not Found", "not found").await,
            },
            WebContent::Directory { root } => {
                write_web_directory(
                    &mut stream,
                    root,
                    share.web_token.as_deref(),
                    path,
                    &request.target,
                )
                .await
            }
        },
        "POST" if path.starts_with("upload?name=") => {
            write_web_upload(
                &mut stream,
                &share,
                path,
                request.content_length,
                &request.body,
            )
            .await
        }
        "POST" => write_web_error(&mut stream, "404 Not Found", "not found").await,
        _ => write_web_error(&mut stream, "405 Method Not Allowed", "method not allowed").await,
    }
}

async fn serve_webrtc_connection(
    mut stream: TcpStream,
    server: Arc<WebRtcServer>,
    web_token: Option<String>,
) -> Result<()> {
    let client_ip = match stream.peer_addr().ok() {
        Some(SocketAddr::V4(address)) if address.ip().is_loopback() => local_web_host().to_string(),
        Some(SocketAddr::V4(address)) => address.ip().to_string(),
        _ => String::new(),
    };
    let mut request = match read_web_request(&mut stream).await {
        Ok(request) => request,
        Err(err) => {
            let message = format!("bad request: {err}");
            return write_web_error(&mut stream, "400 Bad Request", &message).await;
        }
    };
    let Some(path) = web_token_path(web_token.as_deref(), &request.target) else {
        return write_web_error(&mut stream, "404 Not Found", "not found").await;
    };

    match request.method.as_str() {
        "GET" if path.is_empty() => {
            let page = WEBRTC_PAGE.replace("__II_CLIENT_IP__", &client_ip);
            write_web_response_with_headers(
                &mut stream,
                "200 OK",
                "text/html; charset=utf-8",
                "Cache-Control: no-store\r\n",
                page.as_bytes(),
            )
            .await
        }
        "POST" if path == "join" => match server.join() {
            Some(peer_id) => {
                write_web_response(
                    &mut stream,
                    "201 Created",
                    "text/plain; charset=utf-8",
                    peer_id.to_string().as_bytes(),
                )
                .await
            }
            None => {
                write_web_error(&mut stream, "503 Service Unavailable", "peer limit reached").await
            }
        },
        "GET" => {
            if let Some(peer_id) = webrtc_single_peer_path(path, "peers") {
                let Some(peers) = server.peers(peer_id) else {
                    return write_web_error(&mut stream, "404 Not Found", "peer not found").await;
                };
                let mut body = String::from("[");
                for (index, peer_id) in peers.iter().enumerate() {
                    if index > 0 {
                        body.push(',');
                    }
                    body.push_str(&peer_id.to_string());
                }
                body.push(']');
                return write_web_response(
                    &mut stream,
                    "200 OK",
                    "application/json; charset=utf-8",
                    body.as_bytes(),
                )
                .await;
            }
            if let Some(peer_id) = webrtc_single_peer_path(path, "signal") {
                return match server.poll(peer_id) {
                    None => write_web_error(&mut stream, "404 Not Found", "peer not found").await,
                    Some(None) => {
                        write_web_response(
                            &mut stream,
                            "204 No Content",
                            "text/plain; charset=utf-8",
                            b"",
                        )
                        .await
                    }
                    Some(Some(signal)) => {
                        write_web_response_with_headers(
                            &mut stream,
                            "200 OK",
                            "application/json; charset=utf-8",
                            &format!("X-II-From: {}\r\n", signal.from),
                            &signal.body,
                        )
                        .await
                    }
                };
            }
            write_web_error(&mut stream, "404 Not Found", "not found").await
        }
        "POST" if webrtc_single_peer_path(path, "heartbeat").is_some() => {
            let peer_id = webrtc_single_peer_path(path, "heartbeat").unwrap();
            if server.heartbeat(peer_id) {
                write_web_response(
                    &mut stream,
                    "204 No Content",
                    "text/plain; charset=utf-8",
                    b"",
                )
                .await
            } else {
                write_web_error(&mut stream, "404 Not Found", "peer not found").await
            }
        }
        "POST" if webrtc_signal_path(path).is_some() => {
            let (from, to) = webrtc_signal_path(path).unwrap();
            let Some(content_length) = request.content_length else {
                return write_web_error(
                    &mut stream,
                    "411 Length Required",
                    "Content-Length is required",
                )
                .await;
            };
            if content_length == 0 {
                return write_web_error(&mut stream, "400 Bad Request", "signal body is missing")
                    .await;
            }
            if content_length > WEBRTC_MAX_SIGNAL_BYTES {
                return write_web_error(
                    &mut stream,
                    "413 Payload Too Large",
                    "signal body is too large",
                )
                .await;
            }
            let body = match read_webrtc_signal_body(
                &mut stream,
                content_length,
                std::mem::take(&mut request.body),
            )
            .await
            {
                Ok(body) if std::str::from_utf8(&body).is_ok() => body,
                Ok(_) => {
                    return write_web_error(
                        &mut stream,
                        "400 Bad Request",
                        "signal body is not UTF-8",
                    )
                    .await;
                }
                Err(err) => {
                    let message = format!("invalid signal body: {err}");
                    return write_web_error(&mut stream, "400 Bad Request", &message).await;
                }
            };
            match server.relay(from, to, body) {
                Ok(()) => {
                    write_web_response(
                        &mut stream,
                        "204 No Content",
                        "text/plain; charset=utf-8",
                        b"",
                    )
                    .await
                }
                Err(WebRtcRelayError::MissingPeer) => {
                    write_web_error(&mut stream, "404 Not Found", "peer not found").await
                }
                Err(WebRtcRelayError::QueueFull) => {
                    write_web_error(&mut stream, "429 Too Many Requests", "signal queue is full")
                        .await
                }
            }
        }
        "POST" => write_web_error(&mut stream, "404 Not Found", "not found").await,
        _ => write_web_error(&mut stream, "405 Method Not Allowed", "method not allowed").await,
    }
}

fn web_request_path<'a>(share: &WebShare, target: &'a str) -> Option<&'a str> {
    web_token_path(share.web_token.as_deref(), target)
}

fn web_token_path<'a>(web_token: Option<&str>, target: &'a str) -> Option<&'a str> {
    let path = target.strip_prefix('/')?;
    match web_token {
        Some(token) => path.strip_prefix(token)?.strip_prefix('/'),
        None => Some(path),
    }
}

fn webrtc_single_peer_path(path: &str, endpoint: &str) -> Option<u64> {
    let query = path.strip_prefix(endpoint)?.strip_prefix("?id=")?;
    parse_webrtc_peer_id(query)
}

fn webrtc_signal_path(path: &str) -> Option<(u64, u64)> {
    let query = path.strip_prefix("signal?")?;
    let mut from = None;
    let mut to = None;
    for part in query.split('&') {
        let (key, value) = part.split_once('=')?;
        match key {
            "from" if from.replace(parse_webrtc_peer_id(value)?).is_none() => {}
            "to" if to.replace(parse_webrtc_peer_id(value)?).is_none() => {}
            _ => return None,
        }
    }
    let (Some(from), Some(to)) = (from, to) else {
        return None;
    };
    (from != to).then_some((from, to))
}

fn parse_webrtc_peer_id(value: &str) -> Option<u64> {
    let peer_id = value.parse().ok()?;
    (peer_id != 0).then_some(peer_id)
}

async fn read_webrtc_signal_body(
    stream: &mut TcpStream,
    content_length: u64,
    mut body: Vec<u8>,
) -> Result<Vec<u8>> {
    let initial_length = u64::try_from(body.len()).context("signal body is too large")?;
    if initial_length > content_length {
        bail!("signal body exceeds Content-Length");
    }
    let remaining = content_length - initial_length;
    let mut body_reader = stream.take(remaining);
    let copied = body_reader
        .read_to_end(&mut body)
        .await
        .context("read signal body")?;
    if u64::try_from(copied).context("signal body is too large")? != remaining {
        bail!("signal body ended early");
    }
    Ok(body)
}

const WEBRTC_PAGE: &str = include_str!("webrtc_page.html");

async fn read_web_request(stream: &mut TcpStream) -> Result<WebRequest> {
    let mut request = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    let header_end = loop {
        if let Some(position) = request.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
        if request.len() >= 16 * 1024 {
            bail!("request headers exceed 16 KiB");
        }
        let read = stream.read(&mut chunk).await.context("read web request")?;
        if read == 0 {
            bail!("request ended before headers");
        }
        request.extend_from_slice(&chunk[..read]);
    };

    let headers =
        std::str::from_utf8(&request[..header_end - 4]).context("request is not UTF-8")?;
    let mut lines = headers.split("\r\n");
    let request_line = lines.next().context("request line is missing")?;
    let mut parts = request_line.split_ascii_whitespace();
    let method = parts
        .next()
        .context("request method is missing")?
        .to_string();
    let target = parts
        .next()
        .context("request target is missing")?
        .to_string();
    let _version = parts.next().context("request version is missing")?;
    if parts.next().is_some() {
        bail!("request line is invalid");
    }

    let mut content_length = None;
    for line in lines {
        let (name, value) = line.split_once(':').context("request header is invalid")?;
        if name.eq_ignore_ascii_case("content-length") {
            let length = value
                .trim()
                .parse::<u64>()
                .context("Content-Length is invalid")?;
            if content_length.replace(length).is_some() {
                bail!("Content-Length is duplicated");
            }
        }
    }

    Ok(WebRequest {
        method,
        target,
        content_length,
        body: request.split_off(header_end),
    })
}

async fn write_web_page(
    stream: &mut TcpStream,
    source: &Source,
    download_name: &str,
    download_qr_svg: &str,
) -> Result<()> {
    let name = html_escape(download_name);
    let body = format!(
        "<!doctype html><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{name}</title><style>body{{margin:0;background:#f5f5f5;color:#171717;font-family:system-ui,-apple-system,BlinkMacSystemFont,\"Segoe UI\",sans-serif}}main{{box-sizing:border-box;width:min(100%,32rem);margin:0 auto;padding:2rem 1.25rem 2.5rem;text-align:center}}svg{{display:block;width:min(72vw,17.5rem);height:auto;margin:0 auto 1.5rem;background:#fff}}h1{{margin:0;overflow-wrap:anywhere;font-size:1.5rem;line-height:1.3}}.meta{{margin:0.75rem 0 1.5rem;color:#555;font-size:1rem}}a,button{{box-sizing:border-box;display:block;width:100%;min-height:3rem;padding:0.75rem 1rem;border:0;border-radius:0.25rem;background:#1769aa;color:#fff;font:inherit;font-weight:600;line-height:1.5;text-align:center;text-decoration:none}}button:disabled{{opacity:.6}}.upload{{display:grid;gap:.75rem;margin-top:1.5rem;padding-top:1.5rem;border-top:1px solid #ccc;text-align:left}}input{{box-sizing:border-box;width:100%;min-height:3rem;padding:.625rem;border:1px solid #999;border-radius:.25rem;background:#fff;color:#171717;font:inherit}}output{{display:grid;gap:.375rem;overflow-wrap:anywhere;color:#555;font-size:.875rem;line-height:1.4}}@media (max-width:30rem){{main{{padding:1.5rem 1rem 2rem}}svg{{width:min(82vw,17.5rem);margin-bottom:1.25rem}}h1{{font-size:1.25rem}}.meta{{margin:0.625rem 0 1.25rem}}}}</style><main>{}<h1>{name}</h1><p class=\"meta\">{}</p><a href=\"download\">Download</a><div class=\"upload\"><input id=\"upload\" type=\"file\" multiple aria-label=\"Upload files\"><button id=\"upload-button\" type=\"button\">Upload</button><output id=\"upload-status\" aria-live=\"polite\"></output></div></main><script>const input=document.getElementById('upload');const button=document.getElementById('upload-button');const status=document.getElementById('upload-status');button.addEventListener('click',async()=>{{const files=[...input.files];if(!files.length)return;button.disabled=true;status.textContent='';for(const file of files){{const row=document.createElement('div');row.textContent=file.name;status.append(row);try{{const response=await fetch('upload?name='+encodeURIComponent(file.name),{{method:'POST',body:file}});const text=await response.text();row.textContent=response.ok?text:file.name+': '+text;}}catch(error){{row.textContent=file.name+': '+error;}}}}button.disabled=false;input.value='';}});</script>",
        download_qr_svg,
        fmt_bytes(source.size),
    );
    write_web_response(
        stream,
        "200 OK",
        "text/html; charset=utf-8",
        body.as_bytes(),
    )
    .await
}

async fn write_web_download(
    stream: &mut TcpStream,
    source: &Source,
    download_name: &str,
) -> Result<()> {
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nContent-Disposition: attachment; filename=\"{}\"\r\nConnection: close\r\n\r\n",
        source.size,
        content_disposition_name(download_name),
    );
    stream
        .write_all(header.as_bytes())
        .await
        .context("write download headers")?;
    let mut file = source.open_file().await?;
    io::copy(&mut file, stream)
        .await
        .context("write download")?;
    stream.shutdown().await.context("finish download")?;
    Ok(())
}

async fn write_web_directory(
    stream: &mut TcpStream,
    root: &Path,
    web_token: Option<&str>,
    path: &str,
    request_target: &str,
) -> Result<()> {
    let Some((target, segments, trailing_slash)) = web_directory_target(root, path).await else {
        return write_web_error(stream, "404 Not Found", "not found").await;
    };
    let metadata = match fs::metadata(&target).await {
        Ok(metadata) => metadata,
        Err(_) => return write_web_error(stream, "404 Not Found", "not found").await,
    };

    if metadata.is_dir() {
        if !path.is_empty() && !trailing_slash {
            return write_web_redirect(stream, &format!("{request_target}/")).await;
        }
        let body = match web_directory_page_body(root, &target, web_token, &segments).await {
            Ok(body) => body,
            Err(_) => return write_web_error(stream, "404 Not Found", "not found").await,
        };
        return write_web_response(
            stream,
            "200 OK",
            "text/html; charset=utf-8",
            body.as_bytes(),
        )
        .await;
    }

    if trailing_slash {
        return write_web_error(stream, "404 Not Found", "not found").await;
    }
    let file = match fs::File::open(&target).await {
        Ok(file) => file,
        Err(_) => return write_web_error(stream, "404 Not Found", "not found").await,
    };
    write_web_file(stream, file, metadata.len()).await
}

async fn web_directory_target(root: &Path, path: &str) -> Option<(PathBuf, Vec<String>, bool)> {
    if path.starts_with('/') || path.contains('?') {
        return None;
    }
    let trailing_slash = path.ends_with('/');
    let path = if trailing_slash {
        &path[..path.len().checked_sub(1)?]
    } else {
        path
    };
    let mut segments = Vec::new();
    if !path.is_empty() {
        for encoded in path.split('/') {
            if encoded.is_empty() {
                return None;
            }
            let segment = percent_decode_str(encoded).decode_utf8().ok()?.into_owned();
            let mut components = Path::new(&segment).components();
            if segment.is_empty()
                || matches!(segment.as_str(), "." | "..")
                || segment.contains(['/', '\\', '\0'])
                || !matches!(components.next(), Some(Component::Normal(_)))
                || components.next().is_some()
            {
                return None;
            }
            segments.push(segment);
        }
    }

    let target = segments
        .iter()
        .fold(root.to_path_buf(), |path, segment| path.join(segment));
    let target = fs::canonicalize(target).await.ok()?;
    target
        .starts_with(root)
        .then_some((target, segments, trailing_slash))
}

async fn web_directory_page_body(
    root: &Path,
    directory: &Path,
    web_token: Option<&str>,
    segments: &[String],
) -> Result<String> {
    let mut entries = fs::read_dir(directory)
        .await
        .with_context(|| format!("read web directory {}", directory.display()))?;
    let mut listed = Vec::new();
    while let Some(entry) = entries
        .next_entry()
        .await
        .context("read web directory entry")?
    {
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str().map(str::to_owned) else {
            continue;
        };
        let target = match fs::canonicalize(entry.path()).await {
            Ok(target) if target.starts_with(root) => target,
            _ => continue,
        };
        let metadata = match fs::metadata(target).await {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        listed.push(WebDirectoryEntry {
            name,
            is_dir: metadata.is_dir(),
            modified: web_directory_modified(&metadata),
            size: if metadata.is_dir() {
                "-".to_string()
            } else {
                fmt_bytes(metadata.len())
            },
        });
    }
    listed.sort_by(|left, right| left.name.cmp(&right.name));

    let mut rows = String::new();
    if !segments.is_empty() {
        let parent = web_directory_href(&segments[..segments.len() - 1], true);
        rows.push_str(&format!("<a href=\"{parent}\">../</a>\n"));
    }
    for entry in listed {
        let mut entry_segments = segments.to_vec();
        entry_segments.push(entry.name.clone());
        let href = web_directory_href(&entry_segments, entry.is_dir);
        let label = if entry.is_dir {
            format!("{}/", entry.name)
        } else {
            entry.name
        };
        rows.push_str(&format!(
            "<a href=\"{href}\">{}</a>  {}  {}\n",
            html_escape(&label),
            entry.modified,
            entry.size,
        ));
    }

    let display_path = if segments.is_empty() {
        "/".to_string()
    } else {
        format!("/{}/", segments.join("/"))
    };
    let title = format!("Index of {display_path}");
    let mut body = String::from(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><style>body{box-sizing:border-box;max-width:58rem;margin:0 auto;padding:1.5rem 1rem 2rem;background:#fff;color:#111;font-family:ui-monospace,SFMono-Regular,Consolas,monospace;font-size:14px;line-height:1.45}h1{margin:0 0 1rem;font-size:1.25rem;font-weight:600;overflow-wrap:anywhere}.upload{display:grid;gap:.625rem;margin:0 0 1.25rem}.upload input,.upload button{box-sizing:border-box;width:100%;min-height:2.75rem;padding:.625rem;border:1px solid #777;border-radius:0;background:#fff;color:#111;font:inherit}.upload button{background:#1769aa;border-color:#1769aa;color:#fff;font-weight:600}.upload button:disabled{opacity:.6}output{display:grid;gap:.375rem;overflow-wrap:anywhere;color:#555}pre{margin:1rem 0;overflow-x:auto;font:inherit}a{color:#0645ad;text-decoration:none}a:hover{text-decoration:underline}@media (max-width:30rem){body{padding:1rem .75rem 1.5rem;font-size:13px}h1{font-size:1.125rem}}</style><base href=\"",
    );
    body.push_str(&html_escape(&web_root_path(web_token)));
    body.push_str("\"><title>");
    body.push_str(&html_escape(&title));
    body.push_str("</title></head><body><h1>");
    body.push_str(&html_escape(&title));
    body.push_str("</h1>");
    body.push_str(web_upload_controls());
    body.push_str("<hr><pre>Name                             Last modified       Size\n----------------------------------------------------------------\n");
    body.push_str(&rows);
    body.push_str("</pre><hr></body></html>");
    Ok(body)
}

fn web_directory_modified(metadata: &std::fs::Metadata) -> String {
    metadata
        .modified()
        .ok()
        .and_then(|time| {
            OffsetDateTime::from(time)
                .format(WEB_DIRECTORY_TIME_FORMAT)
                .ok()
        })
        .unwrap_or_else(|| "-".to_string())
}

fn web_directory_href(segments: &[String], is_dir: bool) -> String {
    if segments.is_empty() {
        return "./".to_string();
    }
    let mut href = segments
        .iter()
        .map(|segment| utf8_percent_encode(segment, NON_ALPHANUMERIC).to_string())
        .collect::<Vec<_>>()
        .join("/");
    if is_dir {
        href.push('/');
    }
    href
}

fn web_upload_controls() -> &'static str {
    "<section class=\"upload\"><input id=\"upload\" type=\"file\" multiple aria-label=\"Upload files\"><button id=\"upload-button\" type=\"button\">Upload</button><output id=\"upload-status\" aria-live=\"polite\"></output></section><script>const input=document.getElementById('upload');const button=document.getElementById('upload-button');const status=document.getElementById('upload-status');button.addEventListener('click',async()=>{const files=[...input.files];if(!files.length)return;button.disabled=true;status.textContent='';for(const file of files){const row=document.createElement('div');row.textContent=file.name;status.append(row);try{const response=await fetch('upload?name='+encodeURIComponent(file.name),{method:'POST',body:file});const text=await response.text();row.textContent=response.ok?text:file.name+': '+text;}catch(error){row.textContent=file.name+': '+error;}}button.disabled=false;input.value='';});</script>"
}

async fn write_web_file(stream: &mut TcpStream, mut file: fs::File, size: u64) -> Result<()> {
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {size}\r\nConnection: close\r\n\r\n"
    );
    stream
        .write_all(header.as_bytes())
        .await
        .context("write web file headers")?;
    io::copy(&mut file, stream)
        .await
        .context("write web file")?;
    stream.shutdown().await.context("finish web file")?;
    Ok(())
}

async fn write_web_redirect(stream: &mut TcpStream, location: &str) -> Result<()> {
    let header = format!(
        "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    stream
        .write_all(header.as_bytes())
        .await
        .context("write web redirect")?;
    stream.shutdown().await.context("finish web redirect")?;
    Ok(())
}

async fn write_web_upload(
    stream: &mut TcpStream,
    share: &WebShare,
    target: &str,
    content_length: Option<u64>,
    initial_body: &[u8],
) -> Result<()> {
    let name = match web_upload_name(target) {
        Ok(name) => name,
        Err(err) => {
            let message = format!("invalid upload name: {err}");
            return write_web_error(stream, "400 Bad Request", &message).await;
        }
    };
    let Some(content_length) = content_length else {
        return write_web_error(stream, "411 Length Required", "Content-Length is required").await;
    };
    let initial_length = u64::try_from(initial_body.len()).context("upload body is too large")?;
    if initial_length > content_length {
        return write_web_error(
            stream,
            "400 Bad Request",
            "upload body exceeds Content-Length",
        )
        .await;
    }

    let (path, temp) = match create_web_upload_file(&share.upload_dir, &name).await {
        Ok(file) => file,
        Err(err) => {
            let message = format!("create upload file: {err}");
            return write_web_error(stream, "500 Internal Server Error", &message).await;
        }
    };
    let mut file = match temp.reopen() {
        Ok(file) => fs::File::from_std(file),
        Err(err) => {
            let message = format!("open upload file: {err}");
            return write_web_error(stream, "500 Internal Server Error", &message).await;
        }
    };
    let remaining = content_length - initial_length;
    let write_result = async {
        file.write_all(initial_body)
            .await
            .context("write upload body")?;
        let mut body = stream.take(remaining);
        let copied = io::copy(&mut body, &mut file)
            .await
            .context("write upload body")?;
        if copied != remaining {
            bail!("upload body ended early");
        }
        file.flush().await.context("flush upload file")?;
        Ok(())
    }
    .await;
    if let Err(err) = write_result {
        drop(file);
        let message = format!("upload failed: {err}");
        return write_web_error(stream, "400 Bad Request", &message).await;
    }
    drop(file);
    if let Err(err) = fs::remove_file(&path).await
        && err.kind() != std::io::ErrorKind::NotFound
    {
        let message = format!("replace upload file: {err}");
        return write_web_error(stream, "500 Internal Server Error", &message).await;
    }
    if let Err(err) = temp.persist(&path) {
        let message = format!("replace upload file: {}", err.error);
        return write_web_error(stream, "500 Internal Server Error", &message).await;
    }

    println!("ii web: uploaded {}", path.display());
    let message = format!(
        "saved: {}",
        path.file_name().unwrap_or_default().to_string_lossy()
    );
    write_web_response(
        stream,
        "201 Created",
        "text/plain; charset=utf-8",
        message.as_bytes(),
    )
    .await
}

fn web_upload_name(target: &str) -> Result<String> {
    let encoded = target
        .strip_prefix("upload?name=")
        .context("upload name is missing")?;
    if encoded.is_empty() || encoded.contains('&') {
        bail!("upload name is invalid");
    }
    let name = percent_decode_str(encoded)
        .decode_utf8()
        .context("upload name is not UTF-8")?
        .into_owned();
    if name.is_empty()
        || matches!(name.as_str(), "." | "..")
        || name.contains(['/', '\\'])
        || name.contains('\0')
    {
        bail!("upload name is invalid");
    }
    Ok(name)
}

async fn create_web_upload_file(upload_dir: &Path, name: &str) -> Result<(PathBuf, NamedTempFile)> {
    fs::create_dir_all(upload_dir)
        .await
        .with_context(|| format!("create upload directory {}", upload_dir.display()))?;
    let temp = NamedTempFile::new_in(upload_dir)
        .with_context(|| format!("create upload file in {}", upload_dir.display()))?;
    Ok((upload_dir.join(name), temp))
}

async fn write_web_response(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &[u8],
) -> Result<()> {
    write_web_response_with_headers(stream, status, content_type, "", body).await
}

async fn write_web_response_with_headers(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    headers: &str,
    body: &[u8],
) -> Result<()> {
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len(),
    );
    stream
        .write_all(header.as_bytes())
        .await
        .context("write web headers")?;
    stream.write_all(body).await.context("write web body")?;
    stream.shutdown().await.context("finish web response")?;
    Ok(())
}

async fn write_web_error(stream: &mut TcpStream, status: &str, message: &str) -> Result<()> {
    write_web_response(
        stream,
        status,
        "text/plain; charset=utf-8",
        message.as_bytes(),
    )
    .await
}

fn content_disposition_name(name: &str) -> String {
    name.replace(['\r', '\n', '\\', '"'], "_")
}

fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn web_qr_svg(url: &str) -> Result<String> {
    const QUIET_ZONE: i32 = 4;
    let code = QrCode::encode_text(url, QrCodeEcc::Low)
        .map_err(|_| anyhow::anyhow!("generate web QR code: URL is too long"))?;
    let size = code.size();
    let view_box = size + QUIET_ZONE * 2;
    let mut path = String::new();
    for y in 0..size {
        for x in 0..size {
            if code.get_module(x, y) {
                path.push_str(&format!("M{} {}h1v1h-1z", x + QUIET_ZONE, y + QUIET_ZONE));
            }
        }
    }
    Ok(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {view_box} {view_box}\" width=\"240\" height=\"240\" role=\"img\" aria-label=\"QR code\"><rect width=\"100%\" height=\"100%\" fill=\"white\"/><path d=\"{path}\" fill=\"black\"/></svg>"
    ))
}

fn web_qr_terminal(url: &str) -> Result<String> {
    const QUIET_ZONE: i32 = 4;
    let code = QrCode::encode_text(url, QrCodeEcc::Low)
        .map_err(|_| anyhow::anyhow!("generate web QR code: URL is too long"))?;
    let size = code.size();
    let width = size + QUIET_ZONE * 2;
    let height = width + width.rem_euclid(2);
    let mut output = String::new();
    for y in (0..height).step_by(2) {
        for x in 0..width {
            let top = web_qr_module(&code, x, y, QUIET_ZONE);
            let bottom = web_qr_module(&code, x, y + 1, QUIET_ZONE);
            let cell = match (top, bottom) {
                (true, true) => "█",
                (true, false) => "▀",
                (false, true) => "▄",
                (false, false) => " ",
            };
            output.push_str(cell);
        }
        output.push('\n');
    }
    Ok(output)
}

fn web_qr_module(code: &QrCode, x: i32, y: i32, quiet_zone: i32) -> bool {
    let size = code.size();
    let x = x - quiet_zone;
    let y = y - quiet_zone;
    x >= 0 && x < size && y >= 0 && y < size && code.get_module(x, y)
}

pub async fn send_with_events(
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
    let endpoint = bind_endpoint(endpoint_policy_for_send(&args)?).await?;

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
    let upload = upload_to_s3(
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

struct S3UploadResult {
    download_url: String,
    delete_url: Option<String>,
    object_key: String,
}

async fn upload_to_s3(
    source: &Source,
    profile: &storage::S3Profile,
    delete_after_recv: bool,
    show_progress: bool,
) -> Result<S3UploadResult> {
    let source_path = source.local_path();
    let source_size = source.size();
    let profile = profile.clone();
    let object_key = match source.content_md5() {
        Some(content_md5) => storage::content_addressed_object_key(&profile.prefix, content_md5),
        None => storage::normalized_object_key(&profile.prefix, &unique_object_id(), source.name()),
    };
    let object_path = profile.s3_path(&object_key);
    tokio::task::spawn_blocking(move || -> Result<S3UploadResult> {
        let bucket = storage::build_bucket(&profile)?;
        if !s3_object_exists(&bucket, &object_path)? {
            let file = std::fs::File::open(&source_path)
                .with_context(|| format!("open source file {}", source_path.display()))?;
            let progress = TransferProgress::new("ii send", show_progress, source_size, 0);
            let mut file = ProgressReader::new(file, progress);
            let status = bucket
                .put_object_stream(&mut file, &object_path)
                .context("upload to S3")?;
            if !(200..300).contains(&status) {
                bail!("S3 upload failed with status {status}");
            }
            file.finish();
        }
        let download_url = bucket
            .presign_get(&object_path, profile.presign_ttl_seconds, None)
            .context("create presigned download url")?;
        let delete_url = if delete_after_recv {
            Some(
                bucket
                    .presign_delete(&object_path, profile.presign_ttl_seconds)
                    .context("create presigned delete url")?,
            )
        } else {
            None
        };
        Ok(S3UploadResult {
            download_url,
            delete_url,
            object_key,
        })
    })
    .await
    .context("upload task")?
}

fn s3_object_exists(bucket: &crate::s3::Client, object_path: &str) -> Result<bool> {
    match bucket.head_object(object_path) {
        Ok((_, code)) if (200..300).contains(&code) => Ok(true),
        Ok((_, 404)) => Ok(false),
        Ok((_, code)) => bail!("S3 object check failed with status {code}"),
        Err(_) => Ok(false),
    }
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
    let upload = upload_to_webdav(&source, &selection.profile, show_progress).await?;
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

struct WebDavUploadResult {
    object_key: String,
}

async fn upload_to_webdav(
    source: &Source,
    profile: &storage::WebDavProfile,
    show_progress: bool,
) -> Result<WebDavUploadResult> {
    let client = storage::build_webdav_client(profile)?;
    let object_key = match source.content_md5() {
        Some(content_md5) => {
            storage::content_addressed_object_key(&profile.remote_dir, content_md5)
        }
        None => {
            storage::normalized_object_key(&profile.remote_dir, &unique_object_id(), source.name())
        }
    };
    ensure_webdav_parent_dirs(&client, &object_key).await?;
    if webdav_object_exists(&client, &object_key).await? {
        return Ok(WebDavUploadResult { object_key });
    }

    let file = source.open_file().await?;
    let progress = Arc::new(Mutex::new(TransferProgress::new(
        "ii send",
        show_progress,
        source.size(),
        0,
    )));
    let progress_for_stream = Arc::clone(&progress);
    let stream = ReaderStream::new(file).inspect_ok(move |bytes| {
        if let Ok(mut progress) = progress_for_stream.lock() {
            progress.advance(bytes.len() as u64);
        }
    });
    let body = reqwest::Body::wrap_stream(stream);
    let response = client
        .start_request(reqwest::Method::PUT, &object_key)
        .await
        .with_context(|| format!("prepare WebDAV upload {object_key}"))?
        .header("content-type", "application/octet-stream")
        .header("content-length", source.size().unwrap_or(0).to_string())
        .body(body)
        .send()
        .await
        .with_context(|| format!("upload WebDAV object {object_key}"))?;
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        bail!("WebDAV upload failed with status {status}");
    }
    if let Ok(mut progress) = progress.lock() {
        progress.finish();
    }
    Ok(WebDavUploadResult { object_key })
}

async fn ensure_webdav_parent_dirs(client: &crate::webdav::Client, object_key: &str) -> Result<()> {
    let mut current = String::new();
    let parts = object_key.trim_matches('/').split('/').collect::<Vec<_>>();
    for part in parts.iter().take(parts.len().saturating_sub(1)) {
        if part.is_empty() {
            continue;
        }
        if !current.is_empty() {
            current.push('/');
        }
        current.push_str(part);
        match client.mkcol(&current).await {
            Ok(status) if (200..300).contains(&status) || matches!(status, 405 | 409) => {}
            Ok(status) => bail!("create WebDAV dir {current} failed with status {status}"),
            Err(err) => return Err(err).with_context(|| format!("create WebDAV dir {current}")),
        }
    }
    Ok(())
}

async fn webdav_object_exists(client: &crate::webdav::Client, object_key: &str) -> Result<bool> {
    let response = client.propfind(object_key).await?;
    match response.status() {
        status if (200..300).contains(&status) => response
            .is_multistatus()
            .with_context(|| format!("parse WebDAV object response for {object_key}")),
        404 => Ok(false),
        status => bail!("check WebDAV object {object_key} failed with status {status}"),
    }
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
    let upload = upload_to_ftp(&source, &selection.profile, show_progress).await?;
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

struct FtpUploadResult {
    object_key: String,
}

async fn upload_to_ftp(
    source: &Source,
    profile: &storage::FtpProfile,
    show_progress: bool,
) -> Result<FtpUploadResult> {
    let object_key = remote_object_key(&profile.remote_dir, source);
    let mut client = ftp_connect(profile).await?;
    let filename = ftp_enter_object_parent(&mut client, &object_key, true).await?;
    if client.size(&filename).await.is_ok() {
        client.quit().await.ok();
        return Ok(FtpUploadResult { object_key });
    }

    let mut source_file = source.open_file().await?;
    let mut stream = client
        .put_with_stream(&filename)
        .await
        .with_context(|| format!("upload FTP object {object_key}"))?;
    let mut progress = TransferProgress::new("ii send", show_progress, source.size(), 0);
    copy_with_progress(&mut source_file, &mut stream, &mut progress)
        .await
        .with_context(|| format!("upload FTP object {object_key}"))?;
    stream.flush().await.context("flush FTP upload")?;
    progress.finish();
    client
        .finalize_put_stream(stream)
        .await
        .with_context(|| format!("finish FTP upload {object_key}"))?;
    client.quit().await.ok();
    Ok(FtpUploadResult { object_key })
}

async fn ftp_connect(profile: &storage::FtpProfile) -> Result<AsyncFtpStream> {
    storage::validate_ftp_profile(profile)?;
    let url = url::Url::parse(profile.url.trim()).context("parse FTP URL")?;
    let host = url.host_str().context("FTP URL is missing host")?;
    let port = url.port().unwrap_or(21);
    let mut client = AsyncFtpStream::connect((host, port))
        .await
        .with_context(|| format!("connect FTP {host}:{port}"))?;
    client
        .login(&profile.username, &profile.password)
        .await
        .context("authenticate FTP")?;
    Ok(client)
}

fn remote_object_key(remote_dir: &str, source: &Source) -> String {
    match source.content_md5() {
        Some(content_md5) => storage::content_addressed_object_key(remote_dir, content_md5),
        None => storage::normalized_object_key(remote_dir, &unique_object_id(), source.name()),
    }
}

fn remote_path_parts(path: &str) -> Result<Vec<&str>> {
    let parts = path
        .trim_matches('/')
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if parts.is_empty() || parts.iter().any(|part| *part == "." || *part == "..") {
        bail!("invalid remote object path {path}");
    }
    Ok(parts)
}

async fn ftp_enter_object_parent(
    client: &mut AsyncFtpStream,
    object_key: &str,
    create: bool,
) -> Result<String> {
    let parts = remote_path_parts(object_key)?;
    client.cwd("/").await.context("enter FTP login root")?;
    for part in parts.iter().take(parts.len().saturating_sub(1)) {
        if client.cwd(part).await.is_ok() {
            continue;
        }
        if !create {
            bail!("FTP remote directory is missing {part}");
        }
        client
            .mkdir(part)
            .await
            .with_context(|| format!("create FTP directory {part}"))?;
        client
            .cwd(part)
            .await
            .with_context(|| format!("enter FTP directory {part}"))?;
    }
    Ok(parts
        .last()
        .expect("remote object has a file name")
        .to_string())
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
    let upload = upload_to_sftp(&source, &selection.profile, show_progress).await?;
    if selection.save_after_success {
        storage::save_config(&selection.path, &selection.config)?;
    }
    let portable = if args.portable_webdav {
        eprintln!("ii send: warning: portable SFTP ticket includes credentials or a private key");
        Some(sftp_portable_credentials(&selection.profile)?)
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

struct SftpUploadResult {
    object_key: String,
}

struct AcceptAnySftpHost;

impl SshClientHandler for AcceptAnySftpHost {
    type Error = anyhow::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &russh::keys::PublicKey,
    ) -> Result<bool, Self::Error> {
        eprintln!(
            "ii sftp: accepting SSH host key {}",
            server_public_key.fingerprint(HashAlg::Sha256)
        );
        Ok(true)
    }
}

struct SftpConnection {
    _handle: ssh_client::Handle<AcceptAnySftpHost>,
    client: SftpSession,
}

async fn sftp_connect(profile: &storage::SftpProfile) -> Result<SftpConnection> {
    storage::validate_sftp_profile(profile)?;
    let config = ssh_client::Config::default();
    let mut handle = ssh_client::connect(
        Arc::new(config),
        (profile.host.as_str(), profile.port),
        AcceptAnySftpHost,
    )
    .await
    .with_context(|| format!("connect SFTP {}:{}", profile.host, profile.port))?;
    let auth = match profile.auth {
        storage::SftpAuth::Password => handle
            .authenticate_password(&profile.username, &profile.password)
            .await
            .context("authenticate SFTP password")?,
        storage::SftpAuth::PrivateKey => {
            let private_key = decode_secret_key(
                &storage::load_sftp_private_key(profile)?,
                profile.private_key_passphrase.as_deref(),
            )
            .context("parse SFTP private key")?;
            let hash_alg = handle
                .best_supported_rsa_hash()
                .await
                .context("negotiate SFTP RSA signature")?
                .flatten();
            handle
                .authenticate_publickey(
                    &profile.username,
                    PrivateKeyWithHashAlg::new(Arc::new(private_key), hash_alg),
                )
                .await
                .context("authenticate SFTP private key")?
        }
    };
    if !auth.success() {
        bail!("SFTP authentication was rejected");
    }
    let channel = handle
        .channel_open_session()
        .await
        .context("open SFTP session channel")?;
    channel
        .request_subsystem(true, "sftp")
        .await
        .context("start SFTP subsystem")?;
    let client = SftpSession::new(channel.into_stream())
        .await
        .context("start SFTP client")?;
    Ok(SftpConnection {
        _handle: handle,
        client,
    })
}

async fn ensure_sftp_parent_dirs(client: &SftpSession, object_key: &str) -> Result<()> {
    let parts = remote_path_parts(object_key)?;
    let mut current = String::new();
    for part in parts.iter().take(parts.len().saturating_sub(1)) {
        if !current.is_empty() {
            current.push('/');
        }
        current.push_str(part);
        if client.try_exists(&current).await? {
            continue;
        }
        client
            .create_dir(&current)
            .await
            .with_context(|| format!("create SFTP directory {current}"))?;
    }
    Ok(())
}

async fn upload_to_sftp(
    source: &Source,
    profile: &storage::SftpProfile,
    show_progress: bool,
) -> Result<SftpUploadResult> {
    let object_key = remote_object_key(&profile.remote_dir, source);
    let connection = sftp_connect(profile).await?;
    ensure_sftp_parent_dirs(&connection.client, &object_key).await?;
    if connection.client.try_exists(&object_key).await? {
        connection.client.close().await.ok();
        return Ok(SftpUploadResult { object_key });
    }
    let mut source_file = source.open_file().await?;
    let mut remote = connection
        .client
        .open_with_flags(
            object_key.clone(),
            OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE,
        )
        .await
        .with_context(|| format!("open SFTP object {object_key}"))?;
    let mut progress = TransferProgress::new("ii send", show_progress, source.size(), 0);
    copy_with_progress(&mut source_file, &mut remote, &mut progress)
        .await
        .with_context(|| format!("upload SFTP object {object_key}"))?;
    remote.flush().await.context("flush SFTP upload")?;
    remote.shutdown().await.context("finish SFTP upload")?;
    progress.finish();
    connection.client.close().await.ok();
    Ok(SftpUploadResult { object_key })
}

fn sftp_portable_credentials(profile: &storage::SftpProfile) -> Result<SftpPortableCredentials> {
    let auth = match profile.auth {
        storage::SftpAuth::Password => SftpPortableAuth::Password {
            password: profile.password.clone(),
        },
        storage::SftpAuth::PrivateKey => SftpPortableAuth::PrivateKey {
            private_key: storage::load_sftp_private_key(profile)?,
            private_key_passphrase: profile.private_key_passphrase.clone(),
        },
    };
    Ok(SftpPortableCredentials {
        host: profile.host.clone(),
        port: profile.port,
        username: profile.username.clone(),
        remote_dir: profile.remote_dir.clone(),
        auth,
    })
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
        payload_kind_name(ticket.kind()),
        ticket.name(),
        ticket
            .size()
            .map(|size| size.to_string())
            .unwrap_or_else(|| "unknown".to_string())
    ));
    if let Some(endpoint) = ticket.endpoint() {
        trace_endpoint_addr("ticket endpoints", endpoint, &trace);
    }
    if let Some(s3) = ticket.s3_route() {
        trace.info(format_args!("ticket s3 object: {}", s3.object_key));
    }

    let out_dir = args
        .out_dir
        .clone()
        .unwrap_or(std::env::current_dir().context("current dir")?);
    let file_target =
        if matches!(ticket.kind(), PayloadKind::File | PayloadKind::Stdin) && !args.stdout {
            let path = out_dir.join(ticket.name());
            let plan = plan_file_receive(&args, &ticket, &path, &trace).await?;
            if plan == FilePlan::Skip {
                trace.info(format_args!("skipped identical file {}", path.display()));
                eprintln!("ii recv: skipped identical file {}", path.display());
                if let Some(s3) = ticket.s3_route() {
                    try_delete_s3(s3.delete_url.clone(), &mut trace).await;
                }
                if let Some(webdav) = ticket.webdav_route() {
                    try_delete_webdav_for_ticket(webdav.clone(), &mut trace).await;
                }
                if let Some(ftp) = ticket.ftp_route() {
                    try_delete_ftp_for_ticket(ftp.clone(), &mut trace).await;
                }
                if let Some(sftp) = ticket.sftp_route() {
                    try_delete_sftp_for_ticket(sftp.clone(), &mut trace).await;
                }
                return Ok(());
            }
            Some((path, plan))
        } else {
            None
        };

    if ticket.s3_route().is_some() {
        return recv_s3(args, ticket, out_dir, file_target, trace, show_progress).await;
    }
    if ticket.webdav_route().is_some() {
        return recv_webdav(args, ticket, out_dir, file_target, trace, show_progress).await;
    }
    if ticket.ftp_route().is_some() {
        return recv_ftp(args, ticket, out_dir, file_target, trace, show_progress).await;
    }
    if ticket.sftp_route().is_some() {
        return recv_sftp(args, ticket, out_dir, file_target, trace, show_progress).await;
    }

    let relay_only = ticket.is_relay_only();
    if relay_only && args.local {
        bail!("--local cannot be used with a relay-only ticket");
    }
    let policy = if relay_only {
        let relay_url = ticket
            .endpoint()
            .and_then(|endpoint| endpoint.relay_urls().next())
            .cloned()
            .context("relay-only ticket is missing its relay URL")?;
        if ticket.is_self_signed_relay_only() {
            EndpointPolicy::SelfSignedRelayOnly(relay_url)
        } else {
            EndpointPolicy::TrustedRelayOnly(relay_url)
        }
    } else if args.local {
        EndpointPolicy::standard(RelayMode::Disabled)
    } else {
        EndpointPolicy::standard(RelayMode::Default)
    };
    let endpoint = bind_endpoint(policy).await?;
    trace.step("bind endpoint");
    if !args.local {
        trace.info("waiting for endpoint to go online");
        endpoint.online().await;
        trace.step("wait online");
    }

    let mut endpoint_addr = ticket
        .endpoint()
        .cloned()
        .context("peer ticket missing endpoint")?;
    if relay_only {
        endpoint_addr =
            relay_only_addr(&endpoint_addr).context("relay-only ticket has no relay address")?;
        trace.info(if ticket.is_self_signed_relay_only() {
            "using self-signed relay-only path"
        } else {
            "using verified relay-only path"
        });
        trace_endpoint_addr("relay-only endpoints", &endpoint_addr, &trace);
    } else if args.local {
        endpoint_addr = filter_local_addrs(endpoint_addr);
        trace_endpoint_addr("local-filtered endpoints", &endpoint_addr, &trace);
    }
    if endpoint_addr.addrs.is_empty() {
        bail!("ticket has no usable addresses for this mode");
    }

    let conn =
        connect_to_sender(&endpoint, endpoint_addr, args.local || relay_only, &trace).await?;
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

    let bytes_written = match ticket.kind() {
        PayloadKind::File | PayloadKind::Stdin => {
            if args.stdout {
                copy_to_stdout(recv, ticket.size(), show_progress).await?
            } else {
                let (path, plan) = file_target.expect("file target exists");
                let resume_from = match plan {
                    FilePlan::Download { resume_from } => resume_from,
                    FilePlan::Skip => 0,
                };
                write_to_file(recv, path, resume_from, ticket.size(), show_progress).await?
            }
        }
        PayloadKind::Dir => {
            if args.stdout {
                bail!("--stdout is not supported for directory tickets");
            }
            extract_tar_stream(recv, out_dir, ticket.size(), show_progress).await?
        }
    };
    trace.step("receive payload");
    trace.info(format_args!("received {} bytes", bytes_written));

    conn.close(0u32.into(), b"done");
    endpoint.close().await;
    trace.finish(bytes_written);
    Ok(())
}

pub async fn recv_with_events(
    args: RecvArgs,
    events: std::sync::mpsc::Sender<TransferEvent>,
) -> Result<()> {
    let _ = events.send(TransferEvent::Started);
    let result = recv(args).await;
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

async fn recv_s3(
    args: RecvArgs,
    ticket: Ticket,
    out_dir: PathBuf,
    file_target: Option<(PathBuf, FilePlan)>,
    mut trace: RecvTrace,
    show_progress: bool,
) -> Result<()> {
    let s3 = ticket
        .s3_route()
        .context("s3 ticket missing route")?
        .clone();
    trace.info("using s3 storage route");
    let bytes_written = match ticket.kind() {
        PayloadKind::File | PayloadKind::Stdin => {
            if args.stdout {
                download_s3_to_stdout(
                    s3.download_url.clone(),
                    ticket.size(),
                    show_progress,
                    &mut trace,
                )
                .await?
            } else {
                let (path, plan) = file_target.expect("file target exists");
                let resume_from = match plan {
                    FilePlan::Download { resume_from } => resume_from,
                    FilePlan::Skip => 0,
                };
                download_s3_to_file(
                    s3.download_url.clone(),
                    path,
                    resume_from,
                    ticket.size(),
                    show_progress,
                    &mut trace,
                )
                .await?
            }
        }
        PayloadKind::Dir => {
            if args.stdout {
                bail!("--stdout is not supported for directory tickets");
            }
            download_s3_tar(
                s3.download_url.clone(),
                out_dir,
                ticket.size(),
                show_progress,
                &mut trace,
            )
            .await?
        }
    };
    trace.step("receive payload");
    trace.info(format_args!("received {} bytes", bytes_written));
    try_delete_s3(s3.delete_url.clone(), &mut trace).await;
    trace.finish(bytes_written);
    Ok(())
}

async fn recv_webdav(
    args: RecvArgs,
    ticket: Ticket,
    out_dir: PathBuf,
    file_target: Option<(PathBuf, FilePlan)>,
    mut trace: RecvTrace,
    show_progress: bool,
) -> Result<()> {
    let webdav = ticket
        .webdav_route()
        .context("webdav ticket missing route")?
        .clone();
    trace.info(format_args!("using webdav object {}", webdav.object_key));
    let (profile, save_after_success) = match &webdav.portable {
        Some(portable) => {
            let profile = webdav_profile_from_portable(portable)?;
            let save = portable_webdav_config(&webdav.profile, &profile)?;
            (profile, Some(save))
        }
        None => {
            let selection = storage::load_or_prompt_webdav_profile_named(&webdav.profile)?;
            let save = selection
                .save_after_success
                .then_some((selection.path.clone(), selection.config.clone()));
            (selection.profile, save)
        }
    };
    let client = storage::build_webdav_client(&profile)?;

    let bytes_written = match ticket.kind() {
        PayloadKind::File | PayloadKind::Stdin => {
            if args.stdout {
                download_webdav_to_stdout(
                    &client,
                    &webdav.object_key,
                    ticket.size(),
                    show_progress,
                    &mut trace,
                )
                .await?
            } else {
                let (path, plan) = file_target.expect("file target exists");
                let resume_from = match plan {
                    FilePlan::Download { resume_from } => resume_from,
                    FilePlan::Skip => 0,
                };
                download_webdav_to_file(
                    &client,
                    &webdav.object_key,
                    path,
                    resume_from,
                    ticket.size(),
                    show_progress,
                    &mut trace,
                )
                .await?
            }
        }
        PayloadKind::Dir => {
            if args.stdout {
                bail!("--stdout is not supported for directory tickets");
            }
            download_webdav_tar(
                &client,
                &webdav.object_key,
                out_dir,
                ticket.size(),
                show_progress,
                &mut trace,
            )
            .await?
        }
    };
    if let Some((path, config)) = save_after_success {
        storage::save_config(&path, &config)?;
    }
    trace.step("receive payload");
    trace.info(format_args!("received {} bytes", bytes_written));
    try_delete_webdav(
        &client,
        &webdav.object_key,
        webdav.delete_after_recv,
        &mut trace,
    )
    .await;
    trace.finish(bytes_written);
    Ok(())
}

async fn try_delete_webdav_for_ticket(webdav: crate::ticket::WebDavTicket, trace: &mut RecvTrace) {
    if !webdav.delete_after_recv {
        return;
    }
    let result = async {
        let (profile, save_after_success) = match &webdav.portable {
            Some(portable) => {
                let profile = webdav_profile_from_portable(portable)?;
                let save = portable_webdav_config(&webdav.profile, &profile)?;
                (profile, Some(save))
            }
            None => {
                let selection = storage::load_or_prompt_webdav_profile_named(&webdav.profile)?;
                let save = selection
                    .save_after_success
                    .then_some((selection.path.clone(), selection.config.clone()));
                (selection.profile, save)
            }
        };
        let client = storage::build_webdav_client(&profile)?;
        try_delete_webdav(&client, &webdav.object_key, true, trace).await;
        if let Some((path, config)) = save_after_success {
            storage::save_config(&path, &config)?;
        }
        Ok::<(), anyhow::Error>(())
    }
    .await;
    if let Err(err) = result {
        trace.info(format_args!("webdav delete skipped: {err:#}"));
    }
}

async fn recv_ftp(
    args: RecvArgs,
    ticket: Ticket,
    out_dir: PathBuf,
    file_target: Option<(PathBuf, FilePlan)>,
    mut trace: RecvTrace,
    show_progress: bool,
) -> Result<()> {
    let ftp = ticket
        .ftp_route()
        .context("ftp ticket missing route")?
        .clone();
    trace.info(format_args!("using FTP object {}", ftp.object_key));
    let (profile, save_after_success) = match &ftp.portable {
        Some(portable) => {
            let profile = ftp_profile_from_portable(portable)?;
            let save = portable_ftp_config(&ftp.profile, &profile)?;
            (profile, Some(save))
        }
        None => {
            let selection = storage::load_or_prompt_ftp_profile_named(&ftp.profile)?;
            let save = selection
                .save_after_success
                .then_some((selection.path.clone(), selection.config.clone()));
            (selection.profile, save)
        }
    };
    let mut client = ftp_connect(&profile).await?;
    let bytes_written = match ticket.kind() {
        PayloadKind::File | PayloadKind::Stdin => {
            if args.stdout {
                download_ftp_to_stdout(
                    &mut client,
                    &ftp.object_key,
                    ticket.size(),
                    show_progress,
                    &mut trace,
                )
                .await?
            } else {
                let (path, plan) = file_target.expect("file target exists");
                let resume_from = match plan {
                    FilePlan::Download { resume_from } => resume_from,
                    FilePlan::Skip => 0,
                };
                download_ftp_to_file(
                    &mut client,
                    &ftp.object_key,
                    path,
                    resume_from,
                    ticket.size(),
                    show_progress,
                    &mut trace,
                )
                .await?
            }
        }
        PayloadKind::Dir => {
            if args.stdout {
                bail!("--stdout is not supported for directory tickets");
            }
            download_ftp_tar(
                &mut client,
                &ftp.object_key,
                out_dir,
                ticket.size(),
                show_progress,
                &mut trace,
            )
            .await?
        }
    };
    if let Some((path, config)) = save_after_success {
        storage::save_config(&path, &config)?;
    }
    trace.step("receive payload");
    trace.info(format_args!("received {} bytes", bytes_written));
    try_delete_ftp(
        &mut client,
        &ftp.object_key,
        ftp.delete_after_recv,
        &mut trace,
    )
    .await;
    client.quit().await.ok();
    trace.finish(bytes_written);
    Ok(())
}

fn ftp_profile_from_portable(portable: &FtpPortableCredentials) -> Result<storage::FtpProfile> {
    let profile = storage::FtpProfile {
        url: portable.url.clone(),
        username: portable.username.clone(),
        password: portable.password.clone(),
        remote_dir: portable.remote_dir.clone(),
    };
    storage::validate_ftp_profile(&profile)?;
    Ok(profile)
}

fn portable_ftp_config(
    profile_name: &str,
    profile: &storage::FtpProfile,
) -> Result<(PathBuf, storage::IiConfig)> {
    let path = storage::default_config_path()?;
    let mut config = storage::load_config(&path)?;
    config
        .storage
        .ftp
        .insert(profile_name.to_string(), profile.clone());
    Ok((path, config))
}

async fn try_delete_ftp_for_ticket(ftp: crate::ticket::FtpTicket, trace: &mut RecvTrace) {
    if !ftp.delete_after_recv {
        return;
    }
    let result = async {
        let (profile, save_after_success) = match &ftp.portable {
            Some(portable) => {
                let profile = ftp_profile_from_portable(portable)?;
                let save = portable_ftp_config(&ftp.profile, &profile)?;
                (profile, Some(save))
            }
            None => {
                let selection = storage::load_or_prompt_ftp_profile_named(&ftp.profile)?;
                let save = selection
                    .save_after_success
                    .then_some((selection.path.clone(), selection.config.clone()));
                (selection.profile, save)
            }
        };
        let mut client = ftp_connect(&profile).await?;
        try_delete_ftp(&mut client, &ftp.object_key, true, trace).await;
        client.quit().await.ok();
        if let Some((path, config)) = save_after_success {
            storage::save_config(&path, &config)?;
        }
        Ok::<(), anyhow::Error>(())
    }
    .await;
    if let Err(err) = result {
        trace.info(format_args!("ftp delete skipped: {err:#}"));
    }
}

async fn download_ftp_to_file(
    client: &mut AsyncFtpStream,
    object_key: &str,
    path: PathBuf,
    resume_from: u64,
    total_size: Option<u64>,
    show_progress: bool,
    trace: &mut RecvTrace,
) -> Result<u64> {
    trace.info(format_args!("download ftp file to {}", path.display()));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await.ok();
    }
    let filename = ftp_enter_object_parent(client, object_key, false).await?;
    let mut append = false;
    if let Ok(offset) = usize::try_from(resume_from)
        && offset > 0
        && client.resume_transfer(offset).await.is_ok()
    {
        append = true;
    }
    let mut response = client
        .retr_as_stream(&filename)
        .await
        .with_context(|| format!("download FTP object {object_key}"))?;
    let mut file = if append {
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
    let completed = if append { resume_from } else { 0 };
    let mut progress = TransferProgress::new("ii recv", show_progress, total_size, completed);
    let bytes = copy_with_progress(&mut response, &mut file, &mut progress)
        .await
        .with_context(|| format!("write destination {}", path.display()))?;
    progress.finish();
    file.flush()
        .await
        .with_context(|| format!("flush destination {}", path.display()))?;
    client
        .finalize_retr_stream(response)
        .await
        .with_context(|| format!("finish FTP download {object_key}"))?;
    Ok(bytes)
}

async fn download_ftp_to_stdout(
    client: &mut AsyncFtpStream,
    object_key: &str,
    total_size: Option<u64>,
    show_progress: bool,
    trace: &mut RecvTrace,
) -> Result<u64> {
    trace.info("download ftp file to stdout");
    let filename = ftp_enter_object_parent(client, object_key, false).await?;
    let mut response = client
        .retr_as_stream(&filename)
        .await
        .with_context(|| format!("download FTP object {object_key}"))?;
    let mut stdout = io::stdout();
    let mut progress = TransferProgress::new("ii recv", show_progress, total_size, 0);
    let bytes = copy_with_progress(&mut response, &mut stdout, &mut progress)
        .await
        .context("write stdout")?;
    progress.finish();
    stdout.flush().await.ok();
    client
        .finalize_retr_stream(response)
        .await
        .with_context(|| format!("finish FTP download {object_key}"))?;
    Ok(bytes)
}

async fn download_ftp_tar(
    client: &mut AsyncFtpStream,
    object_key: &str,
    out_dir: PathBuf,
    total_size: Option<u64>,
    show_progress: bool,
    trace: &mut RecvTrace,
) -> Result<u64> {
    trace.info(format_args!("download ftp tar to {}", out_dir.display()));
    fs::create_dir_all(&out_dir)
        .await
        .with_context(|| format!("create output dir {}", out_dir.display()))?;
    let filename = ftp_enter_object_parent(client, object_key, false).await?;
    let mut response = client
        .retr_as_stream(&filename)
        .await
        .with_context(|| format!("download FTP object {object_key}"))?;
    let temp = NamedTempFile::new().context("create temp tar")?;
    let temp_path = temp.path().to_path_buf();
    let mut file = fs::File::from_std(temp.reopen().context("reopen temp tar")?);
    let mut progress = TransferProgress::new("ii recv", show_progress, total_size, 0);
    let bytes = copy_with_progress(&mut response, &mut file, &mut progress)
        .await
        .context("buffer ftp tar")?;
    progress.finish();
    file.flush().await.context("flush temp tar")?;
    client
        .finalize_retr_stream(response)
        .await
        .with_context(|| format!("finish FTP download {object_key}"))?;
    let extract_path = out_dir.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let file = std::fs::File::open(&temp_path).context("open tar")?;
        let mut archive = tar::Archive::new(file);
        archive.unpack(&extract_path).context("unpack tar")?;
        Ok(())
    })
    .await
    .context("extract ftp tar task")??;
    Ok(bytes)
}

async fn try_delete_ftp(
    client: &mut AsyncFtpStream,
    object_key: &str,
    delete_after_recv: bool,
    trace: &mut RecvTrace,
) {
    if !delete_after_recv {
        return;
    }
    let result = async {
        let filename = ftp_enter_object_parent(client, object_key, false).await?;
        client
            .rm(&filename)
            .await
            .with_context(|| format!("delete FTP object {object_key}"))?;
        Ok::<(), anyhow::Error>(())
    }
    .await;
    match result {
        Ok(()) => trace.info("ftp delete requested after receive"),
        Err(err) => trace.info(format_args!("ftp delete ignored: {err:#}")),
    }
}

struct PortableSftpProfile {
    profile: storage::SftpProfile,
    _private_key: Option<NamedTempFile>,
    private_key_material: Option<String>,
}

async fn recv_sftp(
    args: RecvArgs,
    ticket: Ticket,
    out_dir: PathBuf,
    file_target: Option<(PathBuf, FilePlan)>,
    mut trace: RecvTrace,
    show_progress: bool,
) -> Result<()> {
    let sftp = ticket
        .sftp_route()
        .context("sftp ticket missing route")?
        .clone();
    trace.info(format_args!("using SFTP object {}", sftp.object_key));
    let mut portable_state = None;
    let (profile, save_after_success) = match &sftp.portable {
        Some(portable) => {
            let state = sftp_profile_from_portable(portable)?;
            let profile = state.profile.clone();
            portable_state = Some(state);
            (profile, None)
        }
        None => {
            let selection = storage::load_or_prompt_sftp_profile_named(&sftp.profile)?;
            let save = selection
                .save_after_success
                .then_some((selection.path.clone(), selection.config.clone()));
            (selection.profile, save)
        }
    };
    let connection = sftp_connect(&profile).await?;
    let bytes_written = match ticket.kind() {
        PayloadKind::File | PayloadKind::Stdin => {
            if args.stdout {
                download_sftp_to_stdout(
                    &connection.client,
                    &sftp.object_key,
                    ticket.size(),
                    show_progress,
                    &mut trace,
                )
                .await?
            } else {
                let (path, plan) = file_target.expect("file target exists");
                let resume_from = match plan {
                    FilePlan::Download { resume_from } => resume_from,
                    FilePlan::Skip => 0,
                };
                download_sftp_to_file(
                    &connection.client,
                    &sftp.object_key,
                    path,
                    resume_from,
                    ticket.size(),
                    show_progress,
                    &mut trace,
                )
                .await?
            }
        }
        PayloadKind::Dir => {
            if args.stdout {
                bail!("--stdout is not supported for directory tickets");
            }
            download_sftp_tar(
                &connection.client,
                &sftp.object_key,
                out_dir,
                ticket.size(),
                show_progress,
                &mut trace,
            )
            .await?
        }
    };
    if let Some(state) = portable_state.as_ref() {
        let (path, config) = portable_sftp_config(
            &sftp.profile,
            &state.profile,
            state.private_key_material.as_deref(),
        )?;
        storage::save_config(&path, &config)?;
    }
    if let Some((path, config)) = save_after_success {
        storage::save_config(&path, &config)?;
    }
    trace.step("receive payload");
    trace.info(format_args!("received {} bytes", bytes_written));
    try_delete_sftp(
        &connection.client,
        &sftp.object_key,
        sftp.delete_after_recv,
        &mut trace,
    )
    .await;
    connection.client.close().await.ok();
    trace.finish(bytes_written);
    Ok(())
}

fn sftp_profile_from_portable(portable: &SftpPortableCredentials) -> Result<PortableSftpProfile> {
    let (
        auth,
        password,
        private_key_path,
        private_key_passphrase,
        private_key,
        private_key_material,
    ) = match &portable.auth {
        SftpPortableAuth::Password { password } => (
            storage::SftpAuth::Password,
            password.clone(),
            None,
            None,
            None,
            None,
        ),
        SftpPortableAuth::PrivateKey {
            private_key,
            private_key_passphrase,
        } => {
            let mut temp = NamedTempFile::new().context("create temporary SFTP private key")?;
            temp.write_all(private_key.as_bytes())
                .context("write temporary SFTP private key")?;
            let path = temp.path().to_path_buf();
            (
                storage::SftpAuth::PrivateKey,
                String::new(),
                Some(path),
                private_key_passphrase.clone(),
                Some(temp),
                Some(private_key.clone()),
            )
        }
    };
    let profile = storage::SftpProfile {
        host: portable.host.clone(),
        port: portable.port,
        username: portable.username.clone(),
        remote_dir: portable.remote_dir.clone(),
        auth,
        password,
        private_key_path,
        private_key_passphrase,
    };
    storage::validate_sftp_profile(&profile)?;
    Ok(PortableSftpProfile {
        profile,
        _private_key: private_key,
        private_key_material,
    })
}

fn portable_sftp_config(
    profile_name: &str,
    profile: &storage::SftpProfile,
    private_key_material: Option<&str>,
) -> Result<(PathBuf, storage::IiConfig)> {
    let path = storage::default_config_path()?;
    let mut config = storage::load_config(&path)?;
    let mut persisted = profile.clone();
    if let Some(private_key) = private_key_material {
        persisted.private_key_path = Some(storage::save_portable_sftp_private_key(
            profile_name,
            private_key,
        )?);
    }
    config
        .storage
        .sftp
        .insert(profile_name.to_string(), persisted);
    Ok((path, config))
}

async fn try_delete_sftp_for_ticket(sftp: crate::ticket::SftpTicket, trace: &mut RecvTrace) {
    if !sftp.delete_after_recv {
        return;
    }
    let result = async {
        let mut portable_state = None;
        let (profile, save_after_success) = match &sftp.portable {
            Some(portable) => {
                let state = sftp_profile_from_portable(portable)?;
                let profile = state.profile.clone();
                portable_state = Some(state);
                (profile, None)
            }
            None => {
                let selection = storage::load_or_prompt_sftp_profile_named(&sftp.profile)?;
                let save = selection
                    .save_after_success
                    .then_some((selection.path.clone(), selection.config.clone()));
                (selection.profile, save)
            }
        };
        let connection = sftp_connect(&profile).await?;
        try_delete_sftp(&connection.client, &sftp.object_key, true, trace).await;
        connection.client.close().await.ok();
        if let Some(state) = portable_state.as_ref() {
            let (path, config) = portable_sftp_config(
                &sftp.profile,
                &state.profile,
                state.private_key_material.as_deref(),
            )?;
            storage::save_config(&path, &config)?;
        }
        if let Some((path, config)) = save_after_success {
            storage::save_config(&path, &config)?;
        }
        Ok::<(), anyhow::Error>(())
    }
    .await;
    if let Err(err) = result {
        trace.info(format_args!("sftp delete skipped: {err:#}"));
    }
}

async fn download_sftp_to_file(
    client: &SftpSession,
    object_key: &str,
    path: PathBuf,
    resume_from: u64,
    total_size: Option<u64>,
    show_progress: bool,
    trace: &mut RecvTrace,
) -> Result<u64> {
    trace.info(format_args!("download sftp file to {}", path.display()));
    remote_path_parts(object_key)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await.ok();
    }
    let mut remote = client
        .open(object_key)
        .await
        .with_context(|| format!("open SFTP object {object_key}"))?;
    let mut append = resume_from > 0;
    if append
        && remote
            .seek(std::io::SeekFrom::Start(resume_from))
            .await
            .is_err()
    {
        append = false;
    }
    let mut file = if append {
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
    let completed = if append { resume_from } else { 0 };
    let mut progress = TransferProgress::new("ii recv", show_progress, total_size, completed);
    let bytes = copy_with_progress(&mut remote, &mut file, &mut progress)
        .await
        .with_context(|| format!("write destination {}", path.display()))?;
    progress.finish();
    file.flush()
        .await
        .with_context(|| format!("flush destination {}", path.display()))?;
    Ok(bytes)
}

async fn download_sftp_to_stdout(
    client: &SftpSession,
    object_key: &str,
    total_size: Option<u64>,
    show_progress: bool,
    trace: &mut RecvTrace,
) -> Result<u64> {
    trace.info("download sftp file to stdout");
    remote_path_parts(object_key)?;
    let mut remote = client
        .open(object_key)
        .await
        .with_context(|| format!("open SFTP object {object_key}"))?;
    let mut stdout = io::stdout();
    let mut progress = TransferProgress::new("ii recv", show_progress, total_size, 0);
    let bytes = copy_with_progress(&mut remote, &mut stdout, &mut progress)
        .await
        .context("write stdout")?;
    progress.finish();
    stdout.flush().await.ok();
    Ok(bytes)
}

async fn download_sftp_tar(
    client: &SftpSession,
    object_key: &str,
    out_dir: PathBuf,
    total_size: Option<u64>,
    show_progress: bool,
    trace: &mut RecvTrace,
) -> Result<u64> {
    trace.info(format_args!("download sftp tar to {}", out_dir.display()));
    remote_path_parts(object_key)?;
    fs::create_dir_all(&out_dir)
        .await
        .with_context(|| format!("create output dir {}", out_dir.display()))?;
    let mut remote = client
        .open(object_key)
        .await
        .with_context(|| format!("open SFTP object {object_key}"))?;
    let temp = NamedTempFile::new().context("create temp tar")?;
    let temp_path = temp.path().to_path_buf();
    let mut file = fs::File::from_std(temp.reopen().context("reopen temp tar")?);
    let mut progress = TransferProgress::new("ii recv", show_progress, total_size, 0);
    let bytes = copy_with_progress(&mut remote, &mut file, &mut progress)
        .await
        .context("buffer sftp tar")?;
    progress.finish();
    file.flush().await.context("flush temp tar")?;
    let extract_path = out_dir.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let file = std::fs::File::open(&temp_path).context("open tar")?;
        let mut archive = tar::Archive::new(file);
        archive.unpack(&extract_path).context("unpack tar")?;
        Ok(())
    })
    .await
    .context("extract sftp tar task")??;
    Ok(bytes)
}

async fn try_delete_sftp(
    client: &SftpSession,
    object_key: &str,
    delete_after_recv: bool,
    trace: &mut RecvTrace,
) {
    if !delete_after_recv {
        return;
    }
    let result = async {
        remote_path_parts(object_key)?;
        client
            .remove_file(object_key)
            .await
            .with_context(|| format!("delete SFTP object {object_key}"))?;
        Ok::<(), anyhow::Error>(())
    }
    .await;
    match result {
        Ok(()) => trace.info("sftp delete requested after receive"),
        Err(err) => trace.info(format_args!("sftp delete ignored: {err:#}")),
    }
}

fn portable_webdav_config(
    profile_name: &str,
    profile: &storage::WebDavProfile,
) -> Result<(PathBuf, storage::IiConfig)> {
    let path = storage::default_config_path()?;
    let mut config = storage::load_config(&path)?;
    config
        .storage
        .webdav
        .insert(profile_name.to_string(), profile.clone());
    Ok((path, config))
}

fn webdav_profile_from_portable(
    portable: &WebDavPortableCredentials,
) -> Result<storage::WebDavProfile> {
    let auth = match portable.auth.as_str() {
        "basic" => storage::WebDavAuth::Basic,
        "digest" => storage::WebDavAuth::Digest,
        other => bail!("unsupported WebDAV auth {other}"),
    };
    Ok(storage::WebDavProfile {
        url: portable.url.clone(),
        username: portable.username.clone(),
        password: portable.password.clone(),
        remote_dir: "ii/".to_string(),
        auth,
    })
}

async fn download_webdav_to_file(
    client: &crate::webdav::Client,
    object_key: &str,
    path: PathBuf,
    resume_from: u64,
    total_size: Option<u64>,
    show_progress: bool,
    trace: &mut RecvTrace,
) -> Result<u64> {
    trace.info(format_args!("download webdav file to {}", path.display()));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await.ok();
    }
    let mut append = resume_from > 0;
    let mut response = webdav_get(client, object_key, resume_from).await?;
    if resume_from > 0 && response.status().as_u16() == 200 {
        append = false;
        response = webdav_get(client, object_key, 0).await?;
    }
    ensure_webdav_success(response.status().as_u16())?;
    let mut file = if append {
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
    let completed = if append { resume_from } else { 0 };
    let mut progress = TransferProgress::new("ii recv", show_progress, total_size, completed);
    let bytes = copy_webdav_response_with_progress(response, &mut file, &mut progress)
        .await
        .with_context(|| format!("write destination {}", path.display()))?;
    progress.finish();
    file.flush()
        .await
        .with_context(|| format!("flush destination {}", path.display()))?;
    Ok(bytes)
}

async fn download_webdav_to_stdout(
    client: &crate::webdav::Client,
    object_key: &str,
    total_size: Option<u64>,
    show_progress: bool,
    trace: &mut RecvTrace,
) -> Result<u64> {
    trace.info("download webdav file to stdout");
    let response = webdav_get(client, object_key, 0).await?;
    ensure_webdav_success(response.status().as_u16())?;
    let mut stdout = io::stdout();
    let mut progress = TransferProgress::new("ii recv", show_progress, total_size, 0);
    let bytes = copy_webdav_response_with_progress(response, &mut stdout, &mut progress)
        .await
        .context("write stdout")?;
    progress.finish();
    stdout.flush().await.ok();
    Ok(bytes)
}

async fn download_webdav_tar(
    client: &crate::webdav::Client,
    object_key: &str,
    out_dir: PathBuf,
    total_size: Option<u64>,
    show_progress: bool,
    trace: &mut RecvTrace,
) -> Result<u64> {
    trace.info(format_args!("download webdav tar to {}", out_dir.display()));
    fs::create_dir_all(&out_dir)
        .await
        .with_context(|| format!("create output dir {}", out_dir.display()))?;
    let response = webdav_get(client, object_key, 0).await?;
    ensure_webdav_success(response.status().as_u16())?;
    let temp = NamedTempFile::new().context("create temp tar")?;
    let temp_path = temp.path().to_path_buf();
    let mut file = fs::File::from_std(temp.reopen().context("reopen temp tar")?);
    let mut progress = TransferProgress::new("ii recv", show_progress, total_size, 0);
    let bytes = copy_webdav_response_with_progress(response, &mut file, &mut progress)
        .await
        .context("buffer webdav tar")?;
    progress.finish();
    file.flush().await.context("flush temp tar")?;
    let extract_path = out_dir.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let file = std::fs::File::open(&temp_path).context("open tar")?;
        let mut archive = tar::Archive::new(file);
        archive.unpack(&extract_path).context("unpack tar")?;
        Ok(())
    })
    .await
    .context("extract webdav tar task")??;
    Ok(bytes)
}

async fn webdav_get(
    client: &crate::webdav::Client,
    object_key: &str,
    resume_from: u64,
) -> Result<reqwest::Response> {
    let mut request = client
        .start_request(reqwest::Method::GET, object_key)
        .await
        .with_context(|| format!("prepare WebDAV download {object_key}"))?;
    if resume_from > 0 {
        request = request.header("range", format!("bytes={resume_from}-"));
    }
    request
        .send()
        .await
        .with_context(|| format!("download WebDAV object {object_key}"))
}

fn ensure_webdav_success(status: u16) -> Result<()> {
    if (200..300).contains(&status) {
        Ok(())
    } else {
        bail!("WebDAV download failed with status {status}")
    }
}

async fn copy_webdav_response_with_progress<W>(
    mut response: reqwest::Response,
    writer: &mut W,
    progress: &mut TransferProgress,
) -> Result<u64>
where
    W: AsyncWrite + Unpin,
{
    let mut written = 0u64;
    while let Some(chunk) = response.chunk().await.context("read WebDAV payload")? {
        writer
            .write_all(&chunk)
            .await
            .context("write WebDAV payload")?;
        let n = chunk.len() as u64;
        written = written.saturating_add(n);
        progress.advance(n);
    }
    Ok(written)
}

async fn try_delete_webdav(
    client: &crate::webdav::Client,
    object_key: &str,
    delete_after_recv: bool,
    trace: &mut RecvTrace,
) {
    if !delete_after_recv {
        return;
    }
    match client.delete(object_key).await {
        Ok(status) if (200..300).contains(&status) => {
            trace.info("webdav delete requested after receive")
        }
        Ok(404) => trace.info("webdav delete ignored: object already missing"),
        Ok(status) => trace.info(format_args!("webdav delete ignored: status {status}")),
        Err(err) => trace.info(format_args!("webdav delete ignored: {err:#}")),
    }
}

async fn try_delete_s3(delete_url: Option<String>, trace: &mut RecvTrace) {
    let Some(delete_url) = delete_url else {
        return;
    };
    let result = tokio::task::spawn_blocking(move || -> Result<()> {
        let response = attohttpc::delete(&delete_url)
            .send()
            .context("delete from S3")?;
        let status = response.status().as_u16();
        if (200..300).contains(&status) || status == 403 || status == 404 {
            Ok(())
        } else {
            bail!("delete returned status {status}");
        }
    })
    .await;
    match result {
        Ok(Ok(())) => trace.info("s3 delete requested after receive"),
        Ok(Err(err)) => trace.info(format_args!("s3 delete ignored: {err:#}")),
        Err(err) => trace.info(format_args!("s3 delete task failed: {err:#}")),
    }
}

async fn download_s3_to_file(
    url: String,
    path: PathBuf,
    resume_from: u64,
    total_size: Option<u64>,
    show_progress: bool,
    trace: &mut RecvTrace,
) -> Result<u64> {
    trace.info(format_args!("download s3 file to {}", path.display()));
    tokio::task::spawn_blocking(move || -> Result<u64> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let mut append = resume_from > 0;
        let mut response = s3_get(&url, resume_from)?;
        if resume_from > 0 && response.status().as_u16() == 200 {
            append = false;
            response = s3_get(&url, 0)?;
        }
        ensure_s3_success(response.status().as_u16())?;
        let mut file = if append {
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .with_context(|| format!("open destination {}", path.display()))?
        } else {
            std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&path)
                .with_context(|| format!("open destination {}", path.display()))?
        };
        let completed = if append { resume_from } else { 0 };
        let mut progress = TransferProgress::new("ii recv", show_progress, total_size, completed);
        let bytes = copy_blocking_with_progress(&mut response, &mut file, &mut progress)
            .with_context(|| format!("write destination {}", path.display()))?;
        progress.finish();
        file.flush()
            .with_context(|| format!("flush destination {}", path.display()))?;
        Ok(bytes)
    })
    .await
    .context("s3 download task")?
}

async fn download_s3_to_stdout(
    url: String,
    total_size: Option<u64>,
    show_progress: bool,
    trace: &mut RecvTrace,
) -> Result<u64> {
    trace.info("download s3 file to stdout");
    tokio::task::spawn_blocking(move || -> Result<u64> {
        let mut response = s3_get(&url, 0)?;
        ensure_s3_success(response.status().as_u16())?;
        let mut stdout = std::io::stdout();
        let mut progress = TransferProgress::new("ii recv", show_progress, total_size, 0);
        let bytes = copy_blocking_with_progress(&mut response, &mut stdout, &mut progress)
            .context("write stdout")?;
        progress.finish();
        stdout.flush().ok();
        Ok(bytes)
    })
    .await
    .context("s3 stdout task")?
}

async fn download_s3_tar(
    url: String,
    out_dir: PathBuf,
    total_size: Option<u64>,
    show_progress: bool,
    trace: &mut RecvTrace,
) -> Result<u64> {
    trace.info(format_args!("download s3 tar to {}", out_dir.display()));
    fs::create_dir_all(&out_dir)
        .await
        .with_context(|| format!("create output dir {}", out_dir.display()))?;
    let temp = NamedTempFile::new().context("create temp tar")?;
    let temp_path = temp.path().to_path_buf();
    let url_for_task = url.clone();
    let bytes = tokio::task::spawn_blocking(move || -> Result<u64> {
        let mut response = s3_get(&url_for_task, 0)?;
        ensure_s3_success(response.status().as_u16())?;
        let mut file = std::fs::File::create(&temp_path).context("create temp tar destination")?;
        let mut progress = TransferProgress::new("ii recv", show_progress, total_size, 0);
        let bytes = copy_blocking_with_progress(&mut response, &mut file, &mut progress)
            .context("buffer s3 tar")?;
        progress.finish();
        file.flush().context("flush temp tar")?;
        Ok(bytes)
    })
    .await
    .context("s3 tar download task")??;

    let extract_path = out_dir.clone();
    let temp_path = temp.path().to_path_buf();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let file = std::fs::File::open(&temp_path).context("open tar")?;
        let mut archive = tar::Archive::new(file);
        archive.unpack(&extract_path).context("unpack tar")?;
        Ok(())
    })
    .await
    .context("extract s3 tar task")??;
    Ok(bytes)
}

fn s3_get(url: &str, resume_from: u64) -> Result<attohttpc::Response> {
    let mut request = attohttpc::get(url);
    if resume_from > 0 {
        request = request.header("range", format!("bytes={resume_from}-"));
    }
    request.send().context("download from S3")
}

fn ensure_s3_success(status: u16) -> Result<()> {
    if (200..300).contains(&status) {
        Ok(())
    } else {
        bail!("S3 download failed with status {status}")
    }
}

struct ProgressReader<R> {
    inner: R,
    progress: TransferProgress,
}

impl<R> ProgressReader<R> {
    fn new(inner: R, progress: TransferProgress) -> Self {
        Self { inner, progress }
    }

    fn finish(&mut self) {
        self.progress.finish();
    }
}

impl<R: Read> Read for ProgressReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        if n > 0 {
            self.progress.advance(n as u64);
        }
        Ok(n)
    }
}

fn copy_blocking_with_progress<R, W>(
    reader: &mut R,
    writer: &mut W,
    progress: &mut TransferProgress,
) -> Result<u64>
where
    R: Read,
    W: Write,
{
    let mut buf = [0u8; 64 * 1024];
    let mut written = 0u64;
    loop {
        let n = reader.read(&mut buf).context("read payload")?;
        if n == 0 {
            break;
        }
        writer.write_all(&buf[..n]).context("write payload")?;
        let n = n as u64;
        written = written.saturating_add(n);
        progress.advance(n);
    }
    Ok(written)
}

async fn bind_endpoint(policy: EndpointPolicy) -> Result<Endpoint> {
    let secret_key = SecretKey::generate();
    let mut builder = Endpoint::builder(presets::Minimal)
        .secret_key(secret_key)
        .alpns(vec![ALPN.to_vec()])
        .relay_mode(policy.relay_mode());
    if policy.is_relay_only() {
        builder = builder.clear_ip_transports().clear_address_lookup();
    }
    if policy.accepts_self_signed_relay() {
        builder = builder.ca_tls_config(accept_self_signed_relay_tls());
    }
    let endpoint = builder.bind().await.context("bind endpoint")?;
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

fn endpoint_policy_for_send(args: &SendArgs) -> Result<EndpointPolicy> {
    if args.local || args.no_relay {
        return Ok(EndpointPolicy::standard(RelayMode::Disabled));
    }
    if let Some(url) = &args.relay {
        return Ok(if args.accept_self_signed_relay {
            EndpointPolicy::SelfSignedRelayOnly(url.clone())
        } else {
            EndpointPolicy::TrustedRelayOnly(url.clone())
        });
    }
    Ok(EndpointPolicy::standard(RelayMode::Default))
}

fn should_wait_online(args: &SendArgs) -> bool {
    !args.local && !args.no_relay
}

enum ServeOutcome {
    Sent,
    Ignored,
}

async fn serve_one(
    conn: iroh::endpoint::Connection,
    source: &Source,
    show_progress: bool,
) -> Result<ServeOutcome> {
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
    source
        .stream_to(&mut send, resume_from, show_progress)
        .await?;
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

    fn local_path(&self) -> PathBuf {
        match &self.backing {
            Backing::Path(path) => path.clone(),
            Backing::Temp(temp) => temp.path().to_path_buf(),
        }
    }

    async fn stream_to<W: AsyncWrite + Unpin>(
        &self,
        out: &mut W,
        resume_from: u64,
        show_progress: bool,
    ) -> Result<()> {
        if resume_from > 0 && self.kind == PayloadKind::Dir {
            bail!("resume is only supported for regular files");
        }
        let mut file = self.open_file().await?;
        if resume_from > 0 {
            file.seek(std::io::SeekFrom::Start(resume_from))
                .await
                .context("seek resume offset")?;
        }
        let mut progress =
            TransferProgress::new("ii send", show_progress, self.size(), resume_from);
        copy_with_progress(&mut file, out, &mut progress)
            .await
            .context("stream payload")?;
        progress.finish();
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

async fn copy_to_stdout(
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
    let mut progress = TransferProgress::new("ii recv", show_progress, total_size, 0);
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
    progress: &mut TransferProgress,
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
    use russh::{Channel, ChannelId, server as ssh_server};
    use russh_sftp::{
        protocol::{Attrs, Data, FileAttributes, Handle, Status, StatusCode, Version},
        server::Handler as SftpServerHandler,
    };
    use std::{
        collections::{HashMap, HashSet},
        net::{Ipv4Addr, SocketAddr},
    };
    use tokio::sync::Mutex as TokioMutex;

    #[test]
    fn ticket_round_trip() {
        let ticket = Ticket::peer(
            EndpointAddr::from_parts(
                SecretKey::generate().public(),
                [TransportAddr::Ip(SocketAddr::from((
                    Ipv4Addr::LOCALHOST,
                    1234,
                )))],
            ),
            "hello.txt".into(),
            PayloadKind::File,
            Some(12),
            Some([1; 16]),
        );
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
        let ticket = test_ticket("same.txt", Some(4), Some(md5_bytes(b"same")));
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
        let ticket = test_ticket("partial.txt", Some(10), Some(md5_bytes(b"partial-all")));
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
        let ticket = test_ticket("changed.txt", Some(3), Some(md5_bytes(b"new")));
        let args = test_recv_args();
        let trace = RecvTrace::new(false);
        let plan = plan_file_receive(&args, &ticket, &path, &trace)
            .await
            .unwrap();
        assert_eq!(plan, FilePlan::Download { resume_from: 0 });
    }

    #[tokio::test]
    async fn web_share_serves_page_and_download() {
        let dir = tempfile::tempdir().unwrap();
        let source_path = dir.path().join("hello.txt");
        std::fs::write(&source_path, b"web payload").unwrap();
        let upload_dir = dir.path().join("ii");
        let share = Arc::new(WebShare {
            content: WebContent::Download {
                source: Source::from_file(source_path, None).await.unwrap(),
                download_name: "hello.txt".to_string(),
                download_qr_svg: web_qr_svg("http://192.168.1.2:3456/download").unwrap(),
            },
            upload_dir: upload_dir.clone(),
            web_token: None,
        });
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            for _ in 0..8 {
                let (stream, _) = listener.accept().await.unwrap();
                let share = Arc::clone(&share);
                tokio::spawn(async move { serve_web_connection(stream, share).await.unwrap() });
            }
        });

        let page = web_request(address, "/").await;
        assert!(page.starts_with(b"HTTP/1.1 200 OK"));
        assert!(
            page.windows(b"hello.txt".len())
                .any(|part| part == b"hello.txt")
        );
        assert!(page.windows(b"<svg".len()).any(|part| part == b"<svg"));
        assert!(
            page.windows(b"name=\"viewport\"".len())
                .any(|part| part == b"name=\"viewport\"")
        );
        assert!(
            page.windows(b"width:min(82vw,17.5rem)".len())
                .any(|part| part == b"width:min(82vw,17.5rem)")
        );
        assert!(
            page.windows(b"href=\"download\"".len())
                .any(|part| part == b"href=\"download\"")
        );
        assert!(
            page.windows(b"type=\"file\" multiple".len())
                .any(|part| part == b"type=\"file\" multiple")
        );
        assert!(
            page.windows(b"fetch('upload?name='".len())
                .any(|part| part == b"fetch('upload?name='")
        );

        let first_upload = web_upload_request(address, "notes.txt", b"first upload").await;
        assert!(first_upload.starts_with(b"HTTP/1.1 201 Created"));
        assert!(first_upload.ends_with(b"saved: notes.txt"));
        assert_eq!(
            std::fs::read(upload_dir.join("notes.txt")).unwrap(),
            b"first upload"
        );

        let second_upload = web_upload_request(address, "notes.txt", b"second upload").await;
        assert!(second_upload.starts_with(b"HTTP/1.1 201 Created"));
        assert!(second_upload.ends_with(b"saved: notes.txt"));
        assert_eq!(
            std::fs::read(upload_dir.join("notes.txt")).unwrap(),
            b"second upload"
        );
        assert!(!upload_dir.join("notes (1).txt").exists());

        let invalid_name = web_upload_request(address, "..%2Fescape.txt", b"invalid").await;
        assert!(invalid_name.starts_with(b"HTTP/1.1 400 Bad Request"));
        assert!(!dir.path().join("escape.txt").exists());

        let missing_length = web_raw_request(
            address,
            b"POST /upload?name=missing.txt HTTP/1.1\r\nHost: test\r\n\r\n",
        )
        .await;
        assert!(missing_length.starts_with(b"HTTP/1.1 411 Length Required"));

        let invalid_length = web_raw_request(
            address,
            b"POST /upload?name=invalid.txt HTTP/1.1\r\nHost: test\r\nContent-Length: nope\r\n\r\n",
        )
        .await;
        assert!(invalid_length.starts_with(b"HTTP/1.1 400 Bad Request"));

        let short_body = web_raw_request(
            address,
            b"POST /upload?name=short.txt HTTP/1.1\r\nHost: test\r\nContent-Length: 8\r\n\r\nshort",
        )
        .await;
        assert!(short_body.starts_with(b"HTTP/1.1 400 Bad Request"));
        assert!(!upload_dir.join("short.txt").exists());

        let download = web_request(address, "/download").await;
        assert!(download.starts_with(b"HTTP/1.1 200 OK"));
        assert!(download.ends_with(b"web payload"));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn web_upload_reports_directory_creation_failure() {
        let dir = tempfile::tempdir().unwrap();
        let source_path = dir.path().join("hello.txt");
        let upload_dir = dir.path().join("not-a-directory");
        std::fs::write(&source_path, b"web payload").unwrap();
        std::fs::write(&upload_dir, b"blocked").unwrap();
        let share = Arc::new(WebShare {
            content: WebContent::Download {
                source: Source::from_file(source_path, None).await.unwrap(),
                download_name: "hello.txt".to_string(),
                download_qr_svg: web_qr_svg("http://192.168.1.2:3456/download").unwrap(),
            },
            upload_dir,
            web_token: None,
        });
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            serve_web_connection(stream, share).await.unwrap();
        });

        let response = web_upload_request(address, "failed.txt", b"upload").await;
        assert!(response.starts_with(b"HTTP/1.1 500 Internal Server Error"));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn web_share_token_requires_the_configured_path_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let source_path = dir.path().join("hello.txt");
        let upload_dir = dir.path().join("custom-uploads");
        let token = "A1b2C3d4E5f6G7h8";
        std::fs::write(&source_path, b"web payload").unwrap();
        let share = Arc::new(WebShare {
            content: WebContent::Download {
                source: Source::from_file(source_path, None).await.unwrap(),
                download_name: "hello.txt".to_string(),
                download_qr_svg: web_qr_svg(&format!("http://192.168.1.2:3456/{token}/download"))
                    .unwrap(),
            },
            upload_dir: upload_dir.clone(),
            web_token: Some(token.to_string()),
        });
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            for _ in 0..9 {
                let (stream, _) = listener.accept().await.unwrap();
                let share = Arc::clone(&share);
                tokio::spawn(async move { serve_web_connection(stream, share).await.unwrap() });
            }
        });

        let page = web_request(address, &format!("/{token}/")).await;
        assert!(page.starts_with(b"HTTP/1.1 200 OK"));
        assert!(
            page.windows(b"href=\"download\"".len())
                .any(|part| part == b"href=\"download\"")
        );
        assert!(
            page.windows(b"fetch('upload?name='".len())
                .any(|part| part == b"fetch('upload?name='")
        );

        let download = web_request(address, &format!("/{token}/download")).await;
        assert!(download.starts_with(b"HTTP/1.1 200 OK"));
        assert!(download.ends_with(b"web payload"));

        let upload = web_upload_request_at(
            address,
            &format!("/{token}/upload?name=notes.txt"),
            b"token upload",
        )
        .await;
        assert!(upload.starts_with(b"HTTP/1.1 201 Created"));
        assert_eq!(
            std::fs::read(upload_dir.join("notes.txt")).unwrap(),
            b"token upload"
        );

        let overwrite = web_upload_request_at(
            address,
            &format!("/{token}/upload?name=notes.txt"),
            b"replacement upload",
        )
        .await;
        assert!(overwrite.starts_with(b"HTTP/1.1 201 Created"));
        assert_eq!(
            std::fs::read(upload_dir.join("notes.txt")).unwrap(),
            b"replacement upload"
        );

        for path in ["/", "/download", &format!("/{token}"), "/not-the-token/"] {
            let response = web_request(address, path).await;
            assert!(response.starts_with(b"HTTP/1.1 404 Not Found"), "{path}");
        }
        let invalid_upload = web_upload_request_at(
            address,
            "/not-the-token/upload?name=blocked.txt",
            b"blocked",
        )
        .await;
        assert!(invalid_upload.starts_with(b"HTTP/1.1 404 Not Found"));
        assert!(!upload_dir.join("blocked.txt").exists());
        server.await.unwrap();
    }

    #[tokio::test]
    async fn web_directory_lists_files_serves_children_and_uploads() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("shared");
        let nested = root.join("nested");
        let upload_dir = temp.path().join("uploads");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(root.join("top file.txt"), b"top").unwrap();
        std::fs::write(nested.join("child.txt"), b"child payload").unwrap();
        let share = Arc::new(WebShare {
            content: WebContent::Directory {
                root: fs::canonicalize(&root).await.unwrap(),
            },
            upload_dir: upload_dir.clone(),
            web_token: None,
        });
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            for _ in 0..9 {
                let (stream, _) = listener.accept().await.unwrap();
                let share = Arc::clone(&share);
                tokio::spawn(async move { serve_web_connection(stream, share).await.unwrap() });
            }
        });

        let root_page = web_request(address, "/").await;
        assert!(root_page.starts_with(b"HTTP/1.1 200 OK"));
        assert!(
            root_page
                .windows(b"Index of /".len())
                .any(|part| part == b"Index of /")
        );
        assert!(
            root_page
                .windows(b"nested/".len())
                .any(|part| part == b"nested/")
        );
        assert!(
            root_page
                .windows(b"top file.txt".len())
                .any(|part| part == b"top file.txt")
        );
        assert!(
            root_page
                .windows(b"type=\"file\" multiple".len())
                .any(|part| part == b"type=\"file\" multiple")
        );
        assert!(
            root_page
                .windows(b"fetch('upload?name='".len())
                .any(|part| part == b"fetch('upload?name='")
        );
        assert!(!root_page.windows(b"<svg".len()).any(|part| part == b"<svg"));

        let redirect = web_request(address, "/nested").await;
        assert!(redirect.starts_with(b"HTTP/1.1 302 Found"));
        assert!(
            redirect
                .windows(b"Location: /nested/".len())
                .any(|part| part == b"Location: /nested/")
        );

        let nested_page = web_request(address, "/nested/").await;
        assert!(nested_page.starts_with(b"HTTP/1.1 200 OK"));
        assert!(
            nested_page
                .windows(b"Index of /nested/".len())
                .any(|part| part == b"Index of /nested/")
        );
        assert!(
            nested_page
                .windows(b">../</a>".len())
                .any(|part| part == b">../</a>")
        );

        let file = web_request(address, "/nested/child.txt").await;
        assert!(file.starts_with(b"HTTP/1.1 200 OK"));
        assert!(file.ends_with(b"child payload"));
        assert!(
            !file
                .windows(b"Content-Disposition".len())
                .any(|part| part == b"Content-Disposition")
        );

        for path in [
            "/%2e%2e/secret.txt",
            "/nested%2fchild.txt",
            "/nested%5cchild.txt",
        ] {
            let response = web_request(address, path).await;
            assert!(response.starts_with(b"HTTP/1.1 404 Not Found"), "{path}");
        }

        let first = web_upload_request(address, "notes.bin", b"first upload").await;
        assert!(first.starts_with(b"HTTP/1.1 201 Created"));
        let overwrite = web_upload_request(address, "notes.bin", b"replacement upload").await;
        assert!(overwrite.starts_with(b"HTTP/1.1 201 Created"));
        assert_eq!(
            std::fs::read(upload_dir.join("notes.bin")).unwrap(),
            b"replacement upload"
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn web_directory_token_scopes_browsing_and_uploads() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("shared");
        let nested = root.join("nested");
        let upload_dir = temp.path().join("uploads");
        let token = "A1b2C3d4E5f6G7h8";
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("child.txt"), b"child").unwrap();
        let share = Arc::new(WebShare {
            content: WebContent::Directory {
                root: fs::canonicalize(&root).await.unwrap(),
            },
            upload_dir: upload_dir.clone(),
            web_token: Some(token.to_string()),
        });
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            for _ in 0..7 {
                let (stream, _) = listener.accept().await.unwrap();
                let share = Arc::clone(&share);
                tokio::spawn(async move { serve_web_connection(stream, share).await.unwrap() });
            }
        });

        let root_page = web_request(address, &format!("/{token}/")).await;
        assert!(root_page.starts_with(b"HTTP/1.1 200 OK"));
        assert!(
            root_page
                .windows(format!("<base href=\"/{token}/\"").len())
                .any(|part| part == format!("<base href=\"/{token}/\"").as_bytes())
        );
        let nested_page = web_request(address, &format!("/{token}/nested/")).await;
        assert!(nested_page.starts_with(b"HTTP/1.1 200 OK"));
        let upload = web_upload_request_at(
            address,
            &format!("/{token}/upload?name=notes.txt"),
            b"token upload",
        )
        .await;
        assert!(upload.starts_with(b"HTTP/1.1 201 Created"));
        assert_eq!(
            std::fs::read(upload_dir.join("notes.txt")).unwrap(),
            b"token upload"
        );
        for path in ["/", "/nested/", "/wrong-token/", &format!("/{token}")] {
            let response = web_request(address, path).await;
            assert!(response.starts_with(b"HTTP/1.1 404 Not Found"), "{path}");
        }
        server.await.unwrap();
    }

    #[tokio::test]
    async fn web_directory_root_requires_an_existing_directory() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("directory");
        let file = temp.path().join("file.txt");
        std::fs::create_dir(&directory).unwrap();
        std::fs::write(&file, b"file").unwrap();

        assert_eq!(
            web_directory_root(temp.path(), None).await.unwrap(),
            fs::canonicalize(temp.path()).await.unwrap()
        );
        assert_eq!(
            web_directory_root(temp.path(), Some(Path::new("directory")))
                .await
                .unwrap(),
            fs::canonicalize(&directory).await.unwrap()
        );
        assert!(web_directory_root(temp.path(), Some(&file)).await.is_err());
        assert!(
            web_directory_root(temp.path(), Some(&temp.path().join("missing")))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn webrtc_serves_a_token_scoped_page_and_relays_signals() {
        let token = "A1b2C3d4E5f6G7h8";
        let state = Arc::new(WebRtcServer::new());
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            for _ in 0..10 {
                let (stream, _) = listener.accept().await.unwrap();
                let state = Arc::clone(&state);
                let token = token.to_string();
                tokio::spawn(async move {
                    serve_webrtc_connection(stream, state, Some(token))
                        .await
                        .unwrap()
                });
            }
        });

        let page = web_request(address, &format!("/{token}/")).await;
        assert!(page.starts_with(b"HTTP/1.1 200 OK"));
        assert!(
            page.windows(b"RTCPeerConnection".len())
                .any(|part| part == b"RTCPeerConnection")
        );
        assert!(
            page.windows(b"iceServers: []".len())
                .any(|part| part == b"iceServers: []")
        );
        assert!(
            page.windows(b"const II_CLIENT_IP = '".len())
                .any(|part| part == b"const II_CLIENT_IP = '")
        );
        assert!(
            !page
                .windows(b"__II_CLIENT_IP__".len())
                .any(|part| part == b"__II_CLIENT_IP__")
        );
        assert!(
            page.windows(b"WebRTC is disabled or blocked".len())
                .any(|part| { part == b"WebRTC is disabled or blocked" })
        );
        assert!(
            page.windows(b"type=\"file\" multiple".len())
                .any(|part| part == b"type=\"file\" multiple")
        );
        assert!(
            page.windows(b"signal?from=${peerId}&to=${to}".len())
                .any(|part| part == b"signal?from=${peerId}&to=${to}")
        );
        assert!(!page.windows(b"<svg".len()).any(|part| part == b"<svg"));
        assert!(!page.windows(b"stun:".len()).any(|part| part == b"stun:"));
        assert!(!page.windows(b"turn:".len()).any(|part| part == b"turn:"));

        let wrong_token = web_request(address, "/").await;
        assert!(wrong_token.starts_with(b"HTTP/1.1 404 Not Found"));

        let first = webrtc_post(address, &format!("/{token}/join"), b"").await;
        let second = webrtc_post(address, &format!("/{token}/join"), b"").await;
        assert!(first.ends_with(b"\r\n\r\n1"));
        assert!(second.ends_with(b"\r\n\r\n2"));

        let peers = web_request(address, &format!("/{token}/peers?id=1")).await;
        assert!(peers.starts_with(b"HTTP/1.1 200 OK"));
        assert!(peers.ends_with(b"[2]"));

        let signal_body = br#"{"type":"offer","description":{"type":"offer"}}"#;
        let signal = webrtc_post(
            address,
            &format!("/{token}/signal?from=1&to=2"),
            signal_body,
        )
        .await;
        assert!(signal.starts_with(b"HTTP/1.1 204 No Content"));

        let delivered = web_request(address, &format!("/{token}/signal?id=2")).await;
        assert!(delivered.starts_with(b"HTTP/1.1 200 OK"));
        assert!(
            delivered
                .windows(b"X-II-From: 1".len())
                .any(|part| part == b"X-II-From: 1")
        );
        assert!(delivered.ends_with(signal_body));

        let empty = web_request(address, &format!("/{token}/signal?id=2")).await;
        assert!(empty.starts_with(b"HTTP/1.1 204 No Content"));

        let missing_peer = webrtc_post(
            address,
            &format!("/{token}/signal?from=1&to=99"),
            signal_body,
        )
        .await;
        assert!(missing_peer.starts_with(b"HTTP/1.1 404 Not Found"));

        let invalid_route = web_request(address, &format!("/{token}/download")).await;
        assert!(invalid_route.starts_with(b"HTTP/1.1 404 Not Found"));
        server.await.unwrap();
    }

    #[test]
    fn webrtc_members_expire_and_bound_pending_signals() {
        let server = WebRtcServer::new();
        let first = server.join().unwrap();
        let second = server.join().unwrap();
        for _ in 0..WEBRTC_MAX_PENDING_SIGNALS {
            server.relay(first, second, b"{}".to_vec()).unwrap();
        }
        assert!(matches!(
            server.relay(first, second, b"{}".to_vec()),
            Err(WebRtcRelayError::QueueFull)
        ));
        {
            let mut state = server.state.lock().unwrap();
            state.peers.get_mut(&first).unwrap().last_seen = Instant::now() - WEBRTC_PEER_TTL;
        }
        assert_eq!(server.peers(first), None);
    }

    #[test]
    fn web_directory_href_encodes_each_path_segment() {
        assert_eq!(web_directory_href(&[], true), "./");
        assert_eq!(
            web_directory_href(&["nested".to_string(), "two words".to_string()], true),
            "nested/two%20words/"
        );
    }

    #[test]
    fn web_root_path_preserves_default_and_token_routes() {
        assert_eq!(web_root_path(None), "/");
        assert_eq!(
            web_root_path(Some("A1b2C3d4E5f6G7h8")),
            "/A1b2C3d4E5f6G7h8/"
        );
    }

    #[test]
    fn web_upload_dir_uses_default_relative_and_absolute_paths() {
        let base = tempfile::tempdir().unwrap();
        let start_dir = base.path();
        let absolute = start_dir.join("absolute-uploads");

        assert_eq!(web_upload_dir(start_dir, None), start_dir.join("ii"));
        assert_eq!(
            web_upload_dir(start_dir, Some(Path::new("relative-uploads"))),
            start_dir.join("relative-uploads")
        );
        assert_eq!(web_upload_dir(start_dir, Some(&absolute)), absolute,);
    }

    #[test]
    fn web_qr_svg_is_self_contained_and_deterministic() {
        let url = "http://192.168.1.2:3456/";
        let first = web_qr_svg(url).unwrap();
        assert_eq!(first, web_qr_svg(url).unwrap());
        assert!(first.starts_with("<svg "));
        assert!(first.contains("viewBox=\"0 0 "));
        assert!(first.contains("<path d=\"M"));
        assert!(!first.contains(url));
        assert!(!first.contains("href="));
        assert!(!first.contains("<script"));
        assert!(!first.contains("<image"));
        assert!(!first.contains("<foreignObject"));
    }

    #[test]
    fn web_qr_terminal_is_self_contained_and_deterministic() {
        let url = "http://192.168.1.2:3456/";
        let first = web_qr_terminal(url).unwrap();
        assert_eq!(first, web_qr_terminal(url).unwrap());
        assert!(!first.contains(url));
        assert!(first.ends_with('\n'));
        assert!(first.contains('█'));
        assert!(first.contains(' '));
        assert!(
            first
                .chars()
                .all(|character| matches!(character, '█' | '▀' | '▄' | ' ' | '\n'))
        );
    }

    #[test]
    fn web_other_hosts_filters_sorts_and_deduplicates() {
        let primary = Ipv4Addr::new(192, 168, 1, 8);
        let hosts = web_other_hosts(
            primary,
            vec![
                Ipv4Addr::UNSPECIFIED,
                Ipv4Addr::LOCALHOST,
                Ipv4Addr::new(172, 17, 0, 1),
                Ipv4Addr::new(10, 0, 0, 2),
                primary,
                Ipv4Addr::new(172, 17, 0, 1),
            ],
        );
        assert_eq!(
            hosts,
            vec![Ipv4Addr::new(10, 0, 0, 2), Ipv4Addr::new(172, 17, 0, 1)]
        );
    }

    #[tokio::test]
    async fn ftp_round_trip_uses_passive_mode_and_deletes_after_receive() {
        let root = tempfile::tempdir().unwrap();
        let port = unused_local_port();
        let ftp_root = root.path().to_path_buf();
        let server = libunftp::ServerBuilder::new(Box::new(move || {
            unftp_sbe_fs::Filesystem::new(ftp_root.clone()).unwrap()
        }))
        .passive_host([127, 0, 0, 1])
        .passive_ports(41000..=41020)
        .build()
        .unwrap();
        let server_task = tokio::spawn(async move {
            let _ = server.listen(format!("127.0.0.1:{port}")).await;
        });
        tokio::time::sleep(Duration::from_millis(100)).await;

        let source_path = root.path().join("source.txt");
        std::fs::write(&source_path, b"ftp payload").unwrap();
        let source = Source::from_file(source_path, None).await.unwrap();
        let profile = storage::FtpProfile {
            url: format!("ftp://127.0.0.1:{port}"),
            username: "user".to_string(),
            password: "pass".to_string(),
            remote_dir: "ii/".to_string(),
        };
        let upload = upload_to_ftp(&source, &profile, false).await.unwrap();

        let destination = root.path().join("received.txt");
        let mut client = ftp_connect(&profile).await.unwrap();
        let mut trace = RecvTrace::new(false);
        let bytes = download_ftp_to_file(
            &mut client,
            &upload.object_key,
            destination.clone(),
            0,
            source.size(),
            false,
            &mut trace,
        )
        .await
        .unwrap();
        assert_eq!(bytes, 11);
        assert_eq!(std::fs::read(&destination).unwrap(), b"ftp payload");
        try_delete_ftp(&mut client, &upload.object_key, true, &mut trace).await;
        client.quit().await.unwrap();
        assert!(!root.path().join(&upload.object_key).exists());
        server_task.abort();
    }

    #[tokio::test]
    async fn sftp_password_round_trip_accepts_host_key_and_deletes_after_receive() {
        let state = Arc::new(TestSftpState::default());
        let port = unused_local_port();
        let config = ssh_server::Config {
            keys: vec![
                russh::keys::PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519)
                    .unwrap(),
            ],
            ..Default::default()
        };
        let mut server = TestSftpServer {
            state: Arc::clone(&state),
        };
        let server_task = tokio::spawn(async move {
            let _ = ssh_server::Server::run_on_address(
                &mut server,
                Arc::new(config),
                ("127.0.0.1", port),
            )
            .await;
        });
        tokio::time::sleep(Duration::from_millis(100)).await;

        let root = tempfile::tempdir().unwrap();
        let source_path = root.path().join("source.txt");
        std::fs::write(&source_path, b"sftp payload").unwrap();
        let source = Source::from_file(source_path, None).await.unwrap();
        let profile = storage::SftpProfile {
            host: "127.0.0.1".to_string(),
            port,
            username: "user".to_string(),
            remote_dir: "ii/".to_string(),
            auth: storage::SftpAuth::Password,
            password: "pass".to_string(),
            private_key_path: None,
            private_key_passphrase: None,
        };
        let upload = upload_to_sftp(&source, &profile, false).await.unwrap();

        let destination = root.path().join("received.txt");
        let connection = sftp_connect(&profile).await.unwrap();
        let mut trace = RecvTrace::new(false);
        let bytes = download_sftp_to_file(
            &connection.client,
            &upload.object_key,
            destination.clone(),
            0,
            source.size(),
            false,
            &mut trace,
        )
        .await
        .unwrap();
        assert_eq!(bytes, 12);
        assert_eq!(std::fs::read(&destination).unwrap(), b"sftp payload");
        try_delete_sftp(&connection.client, &upload.object_key, true, &mut trace).await;
        connection.client.close().await.unwrap();
        assert!(!state.files.lock().await.contains_key(&upload.object_key));
        server_task.abort();
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

    fn unused_local_port() -> u16 {
        let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        listener.local_addr().unwrap().port()
    }

    async fn web_request(address: SocketAddr, path: &str) -> Vec<u8> {
        web_raw_request(
            address,
            format!("GET {path} HTTP/1.1\r\nHost: test\r\n\r\n").as_bytes(),
        )
        .await
    }

    async fn web_upload_request(address: SocketAddr, name: &str, body: &[u8]) -> Vec<u8> {
        web_upload_request_at(address, &format!("/upload?name={name}"), body).await
    }

    async fn web_upload_request_at(address: SocketAddr, path: &str, body: &[u8]) -> Vec<u8> {
        let mut request = format!(
            "POST {path} HTTP/1.1\r\nHost: test\r\nContent-Length: {}\r\n\r\n",
            body.len()
        )
        .into_bytes();
        request.extend_from_slice(body);
        web_raw_request(address, &request).await
    }

    async fn webrtc_post(address: SocketAddr, path: &str, body: &[u8]) -> Vec<u8> {
        let mut request = format!(
            "POST {path} HTTP/1.1\r\nHost: test\r\nContent-Length: {}\r\n\r\n",
            body.len()
        )
        .into_bytes();
        request.extend_from_slice(body);
        web_raw_request(address, &request).await
    }

    async fn web_raw_request(address: SocketAddr, request: &[u8]) -> Vec<u8> {
        tokio::time::timeout(Duration::from_secs(5), async {
            let mut stream = TcpStream::connect(address).await.unwrap();
            stream.write_all(request).await.unwrap();
            stream.shutdown().await.unwrap();
            let mut response = Vec::new();
            stream.read_to_end(&mut response).await.unwrap();
            response
        })
        .await
        .unwrap()
    }

    #[derive(Default)]
    struct TestSftpState {
        files: TokioMutex<HashMap<String, Vec<u8>>>,
        dirs: TokioMutex<HashSet<String>>,
    }

    struct TestSftpServer {
        state: Arc<TestSftpState>,
    }

    impl ssh_server::Server for TestSftpServer {
        type Handler = TestSshSession;

        fn new_client(&mut self, _: Option<SocketAddr>) -> Self::Handler {
            TestSshSession {
                state: Arc::clone(&self.state),
                channels: Arc::new(TokioMutex::new(HashMap::new())),
            }
        }
    }

    struct TestSshSession {
        state: Arc<TestSftpState>,
        channels: Arc<TokioMutex<HashMap<ChannelId, Channel<ssh_server::Msg>>>>,
    }

    impl ssh_server::Handler for TestSshSession {
        type Error = anyhow::Error;

        async fn auth_password(
            &mut self,
            _user: &str,
            _password: &str,
        ) -> Result<ssh_server::Auth, Self::Error> {
            Ok(ssh_server::Auth::Accept)
        }

        async fn channel_open_session(
            &mut self,
            channel: Channel<ssh_server::Msg>,
            _session: &mut ssh_server::Session,
        ) -> Result<bool, Self::Error> {
            self.channels.lock().await.insert(channel.id(), channel);
            Ok(true)
        }

        async fn subsystem_request(
            &mut self,
            channel_id: ChannelId,
            name: &str,
            session: &mut ssh_server::Session,
        ) -> Result<(), Self::Error> {
            if name != "sftp" {
                session.channel_failure(channel_id)?;
                return Ok(());
            }
            let channel = self
                .channels
                .lock()
                .await
                .remove(&channel_id)
                .context("missing SFTP test channel")?;
            session.channel_success(channel_id)?;
            russh_sftp::server::run(
                channel.into_stream(),
                TestSftpHandler {
                    state: Arc::clone(&self.state),
                },
            )
            .await;
            Ok(())
        }
    }

    struct TestSftpHandler {
        state: Arc<TestSftpState>,
    }

    impl SftpServerHandler for TestSftpHandler {
        type Error = StatusCode;

        fn unimplemented(&self) -> Self::Error {
            StatusCode::OpUnsupported
        }

        async fn init(
            &mut self,
            _version: u32,
            _extensions: HashMap<String, String>,
        ) -> Result<Version, Self::Error> {
            Ok(Version::new())
        }

        async fn open(
            &mut self,
            id: u32,
            filename: String,
            flags: OpenFlags,
            _attrs: FileAttributes,
        ) -> Result<Handle, Self::Error> {
            let mut files = self.state.files.lock().await;
            if flags.contains(OpenFlags::TRUNCATE) {
                files.insert(filename.clone(), Vec::new());
            } else if flags.contains(OpenFlags::CREATE) {
                files.entry(filename.clone()).or_default();
            } else if !files.contains_key(&filename) {
                return Err(StatusCode::NoSuchFile);
            }
            Ok(Handle {
                id,
                handle: filename,
            })
        }

        async fn close(&mut self, id: u32, _handle: String) -> Result<Status, Self::Error> {
            Ok(test_sftp_status(id))
        }

        async fn read(
            &mut self,
            id: u32,
            handle: String,
            offset: u64,
            len: u32,
        ) -> Result<Data, Self::Error> {
            let files = self.state.files.lock().await;
            let bytes = files.get(&handle).ok_or(StatusCode::NoSuchFile)?;
            let start = usize::try_from(offset).map_err(|_| StatusCode::Eof)?;
            if start >= bytes.len() {
                return Err(StatusCode::Eof);
            }
            let end = start.saturating_add(len as usize).min(bytes.len());
            Ok(Data {
                id,
                data: bytes[start..end].to_vec(),
            })
        }

        async fn write(
            &mut self,
            id: u32,
            handle: String,
            offset: u64,
            data: Vec<u8>,
        ) -> Result<Status, Self::Error> {
            let start = usize::try_from(offset).map_err(|_| StatusCode::Failure)?;
            let mut files = self.state.files.lock().await;
            let bytes = files.get_mut(&handle).ok_or(StatusCode::NoSuchFile)?;
            let end = start.saturating_add(data.len());
            if bytes.len() < end {
                bytes.resize(end, 0);
            }
            bytes[start..end].copy_from_slice(&data);
            Ok(test_sftp_status(id))
        }

        async fn stat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
            if let Some(bytes) = self.state.files.lock().await.get(&path) {
                let mut attrs = FileAttributes::empty();
                attrs.size = Some(bytes.len() as u64);
                return Ok(Attrs { id, attrs });
            }
            if self.state.dirs.lock().await.contains(&path) {
                return Ok(Attrs {
                    id,
                    attrs: FileAttributes::default(),
                });
            }
            Err(StatusCode::NoSuchFile)
        }

        async fn mkdir(
            &mut self,
            id: u32,
            path: String,
            _attrs: FileAttributes,
        ) -> Result<Status, Self::Error> {
            self.state.dirs.lock().await.insert(path);
            Ok(test_sftp_status(id))
        }

        async fn remove(&mut self, id: u32, filename: String) -> Result<Status, Self::Error> {
            self.state.files.lock().await.remove(&filename);
            Ok(test_sftp_status(id))
        }
    }

    fn test_sftp_status(id: u32) -> Status {
        Status {
            id,
            status_code: StatusCode::Ok,
            error_message: String::new(),
            language_tag: String::new(),
        }
    }
}
