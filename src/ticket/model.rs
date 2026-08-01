use iroh::EndpointAddr;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TicketCommon {
    pub name: String,
    pub kind: PayloadKind,
    pub size: Option<u64>,
    pub content_md5: Option<[u8; 16]>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PeerTicket {
    pub version: u8,
    pub endpoint: EndpointAddr,
    pub common: TicketCommon,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RelayOnlyTicket {
    pub version: u8,
    pub endpoint: EndpointAddr,
    pub common: TicketCommon,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct S3Ticket {
    pub version: u8,
    pub download_url: String,
    pub delete_url: Option<String>,
    pub object_key: String,
    pub common: TicketCommon,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WebDavPortableCredentials {
    pub url: String,
    pub username: String,
    pub password: String,
    pub auth: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WebDavTicket {
    pub version: u8,
    pub profile: String,
    pub object_key: String,
    #[serde(default)]
    pub delete_after_recv: bool,
    pub portable: Option<WebDavPortableCredentials>,
    pub common: TicketCommon,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FtpPortableCredentials {
    pub url: String,
    pub username: String,
    pub password: String,
    pub remote_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FtpTicket {
    pub version: u8,
    pub profile: String,
    pub object_key: String,
    #[serde(default)]
    pub delete_after_recv: bool,
    pub portable: Option<FtpPortableCredentials>,
    pub common: TicketCommon,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SftpPortableAuth {
    Password {
        password: String,
    },
    PrivateKey {
        private_key: String,
        private_key_passphrase: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SftpPortableCredentials {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub remote_dir: String,
    pub auth: SftpPortableAuth,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SftpTicket {
    pub version: u8,
    pub profile: String,
    pub object_key: String,
    #[serde(default)]
    pub delete_after_recv: bool,
    pub portable: Option<SftpPortableCredentials>,
    pub common: TicketCommon,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TunnelRelayMode {
    Default,
    SelfSignedRelayOnly,
    TrustedRelayOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TunnelTicket {
    pub version: u8,
    pub endpoint: EndpointAddr,
    pub access_key: [u8; 32],
    pub relay_mode: TunnelRelayMode,
}

// Keep the variant order stable: postcard encodes the discriminant by position.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Ticket {
    Peer(PeerTicket),
    S3(S3Ticket),
    WebDav(WebDavTicket),
    RelayOnly(RelayOnlyTicket),
    TrustedRelayOnly(RelayOnlyTicket),
    Ftp(FtpTicket),
    Sftp(SftpTicket),
    Tunnel(TunnelTicket),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PayloadKind {
    File,
    Dir,
    Stdin,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumeRequest {
    pub resume_from: u64,
}
