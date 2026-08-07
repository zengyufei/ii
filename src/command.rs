use std::{
    net::{IpAddr, SocketAddr},
    path::PathBuf,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChecksumAlgorithm {
    #[default]
    Md5,
    Sha256,
}

impl ChecksumAlgorithm {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Md5 => "md5",
            Self::Sha256 => "sha256",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SymlinkPolicy {
    #[default]
    Follow,
    Preserve,
    Reject,
}

#[derive(Debug)]
pub struct Cli {
    pub command: Command,
}

#[derive(Debug)]
pub enum Command {
    Send(SendArgs),
    Watch(WatchArgs),
    Queue(QueueArgs),
    Web(WebArgs),
    Dav(DavArgs),
    Socks5(Socks5Args),
    Webrtc(WebrtcArgs),
    Tunnel(TunnelArgs),
    Recv(RecvArgs),
    Relay(RelayArgs),
    Discover(DiscoverArgs),
    Doctor(DoctorArgs),
    Version,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct DoctorArgs {
    pub nat: bool,
}

#[derive(Debug, Clone, Default)]
pub struct SendArgs {
    pub path: Option<PathBuf>,
    pub extra_paths: Vec<PathBuf>,
    pub name: Option<String>,
    pub include: Vec<String>,
    pub exclude: Vec<String>,
    pub rate: Option<u64>,
    pub checksum: Option<ChecksumAlgorithm>,
    pub preserve_metadata: bool,
    pub symlinks: SymlinkPolicy,
    pub quic_port: Option<u16>,
    pub json: bool,
    pub keep_alive: bool,
    pub copy: bool,
    pub output: Option<PathBuf>,
    pub s3: bool,
    pub r2: bool,
    pub azure: bool,
    pub ftp: bool,
    pub delete_after_recv: bool,
    pub profile: Option<String>,
    pub webdav: bool,
    pub sftp: bool,
    pub web: bool,
    pub web_port: Option<u16>,
    pub web_bind: Option<IpAddr>,
    pub web_token: Option<String>,
    pub web_upload: bool,
    pub web_upload_dir: Option<PathBuf>,
    pub portable_webdav: bool,
    pub local: bool,
    pub relay: Vec<iroh::RelayUrl>,
    pub accept_self_signed_relay: bool,
    pub no_relay: bool,
}

#[derive(Debug, Clone)]
pub struct WatchArgs {
    pub dir: PathBuf,
    pub interval: std::time::Duration,
    pub stabilize: std::time::Duration,
    pub send: SendArgs,
}

#[derive(Debug, Clone)]
pub struct QueueArgs {
    pub paths: Vec<PathBuf>,
    pub after: Option<std::time::Duration>,
    pub every: Option<std::time::Duration>,
    pub send: SendArgs,
}

#[derive(Debug, Clone, Default)]
pub struct WebArgs {
    pub dir: Option<PathBuf>,
    pub web_port: Option<u16>,
    pub web_bind: Option<IpAddr>,
    pub web_token: Option<String>,
    pub web_upload: bool,
    pub web_upload_dir: Option<PathBuf>,
    pub once: bool,
}

#[derive(Debug, Clone, Default)]
pub struct WebrtcArgs {
    pub web_port: Option<u16>,
    pub web_bind: Option<IpAddr>,
    pub web_token: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct DavArgs {
    pub dir: Option<PathBuf>,
    pub web_port: Option<u16>,
    pub web_bind: Option<IpAddr>,
    pub web_token: Option<String>,
    pub read_only: bool,
    pub username: Option<String>,
    pub password: Option<String>,
    pub tls: bool,
    pub domain: Option<String>,
    pub cert: Option<PathBuf>,
    pub key: Option<PathBuf>,
}

#[derive(Debug, Clone, Default)]
pub struct Socks5Args {
    pub port: Option<u16>,
    pub bind: Option<IpAddr>,
    pub username: Option<String>,
    pub password: Option<String>,
}

#[derive(Debug, Clone)]
pub enum TunnelArgs {
    Serve {
        target: String,
        relay: Option<iroh::RelayUrl>,
        accept_self_signed_relay: bool,
    },
    Connect {
        ticket: String,
        listen: Option<SocketAddr>,
    },
}

#[derive(Debug, Clone)]
pub struct RecvArgs {
    pub ticket: String,
    pub out_dir: Option<PathBuf>,
    pub stdout: bool,
    pub overwrite: bool,
    pub resume: bool,
    pub local: bool,
    pub trace: bool,
    pub json: bool,
    pub checksum: Option<ChecksumAlgorithm>,
    pub quic_port: Option<u16>,
}

#[derive(Debug, Clone, Default)]
pub struct DiscoverArgs {
    pub json: bool,
}

#[derive(Debug, Clone)]
pub struct RelayArgs {
    pub tls: bool,
    pub domain: Option<String>,
    pub cert: Option<PathBuf>,
    pub key: Option<PathBuf>,
    pub port: Option<u16>,
    pub bind: Option<IpAddr>,
}
