use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use iroh::EndpointAddr;
use serde::{Deserialize, Serialize};

const PREFIX: &str = "ii1";

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
pub enum Ticket {
    Peer(PeerTicket),
    S3(S3Ticket),
    WebDav(WebDavTicket),
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

impl Ticket {
    pub fn peer(
        endpoint: EndpointAddr,
        name: String,
        kind: PayloadKind,
        size: Option<u64>,
        content_md5: Option<[u8; 16]>,
    ) -> Self {
        Ticket::Peer(PeerTicket {
            version: 2,
            endpoint,
            common: TicketCommon {
                name,
                kind,
                size,
                content_md5,
            },
        })
    }

    pub fn s3(
        download_url: String,
        delete_url: Option<String>,
        object_key: String,
        name: String,
        kind: PayloadKind,
        size: Option<u64>,
        content_md5: Option<[u8; 16]>,
    ) -> Self {
        Ticket::S3(S3Ticket {
            version: 3,
            download_url,
            delete_url,
            object_key,
            common: TicketCommon {
                name,
                kind,
                size,
                content_md5,
            },
        })
    }

    pub fn webdav(
        profile: String,
        object_key: String,
        delete_after_recv: bool,
        portable: Option<WebDavPortableCredentials>,
        name: String,
        kind: PayloadKind,
        size: Option<u64>,
        content_md5: Option<[u8; 16]>,
    ) -> Self {
        Ticket::WebDav(WebDavTicket {
            version: 4,
            profile,
            object_key,
            delete_after_recv,
            portable,
            common: TicketCommon {
                name,
                kind,
                size,
                content_md5,
            },
        })
    }

    pub fn encode(&self) -> Result<String> {
        let bytes = postcard::to_stdvec(self).context("serialize ticket")?;
        Ok(format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(bytes)))
    }

    pub fn decode(raw: &str) -> Result<Self> {
        let body = raw
            .strip_prefix(PREFIX)
            .ok_or_else(|| anyhow::anyhow!("ticket must start with {PREFIX}"))?;
        let bytes = URL_SAFE_NO_PAD
            .decode(body.as_bytes())
            .context("decode ticket")?;
        if let Ok(ticket) = postcard::from_bytes::<Ticket>(&bytes) {
            match &ticket {
                Ticket::Peer(peer) if peer.version == 2 => return Ok(ticket),
                Ticket::S3(s3) if s3.version == 3 => return Ok(ticket),
                Ticket::WebDav(webdav) if webdav.version == 4 => return Ok(ticket),
                Ticket::Peer(peer) if peer.version != 2 => {
                    bail!("unsupported peer ticket version {}", peer.version)
                }
                Ticket::S3(s3) if s3.version != 3 => {
                    bail!("unsupported s3 ticket version {}", s3.version)
                }
                Ticket::WebDav(webdav) if webdav.version != 4 => {
                    bail!("unsupported webdav ticket version {}", webdav.version)
                }
                _ => {}
            }
        }
        if let Ok(legacy_s3) = postcard::from_bytes::<LegacyS3Ticket>(&bytes) {
            if legacy_s3.version == 3 {
                return Ok(Ticket::S3(S3Ticket {
                    version: 3,
                    download_url: legacy_s3.download_url,
                    delete_url: None,
                    object_key: legacy_s3.object_key,
                    common: legacy_s3.common,
                }));
            }
        }
        let legacy: LegacyTicket = postcard::from_bytes(&bytes).context("parse ticket")?;
        if legacy.version != 1 && legacy.version != 2 {
            bail!("unsupported ticket version {}", legacy.version);
        }
        Ok(Ticket::peer(
            legacy.endpoint,
            legacy.name,
            legacy.kind,
            legacy.size,
            legacy.content_md5,
        ))
    }

    pub fn common(&self) -> &TicketCommon {
        match self {
            Ticket::Peer(ticket) => &ticket.common,
            Ticket::S3(ticket) => &ticket.common,
            Ticket::WebDav(ticket) => &ticket.common,
        }
    }

    pub fn endpoint(&self) -> Option<&EndpointAddr> {
        match self {
            Ticket::Peer(ticket) => Some(&ticket.endpoint),
            Ticket::S3(_) | Ticket::WebDav(_) => None,
        }
    }

    pub fn s3_route(&self) -> Option<&S3Ticket> {
        match self {
            Ticket::Peer(_) | Ticket::WebDav(_) => None,
            Ticket::S3(ticket) => Some(ticket),
        }
    }

    pub fn webdav_route(&self) -> Option<&WebDavTicket> {
        match self {
            Ticket::Peer(_) | Ticket::S3(_) => None,
            Ticket::WebDav(ticket) => Some(ticket),
        }
    }

    pub fn name(&self) -> &str {
        &self.common().name
    }

    pub fn kind(&self) -> PayloadKind {
        self.common().kind
    }

    pub fn size(&self) -> Option<u64> {
        self.common().size
    }

    pub fn content_md5(&self) -> Option<[u8; 16]> {
        self.common().content_md5
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LegacyTicket {
    pub version: u8,
    pub endpoint: EndpointAddr,
    pub name: String,
    pub kind: PayloadKind,
    pub size: Option<u64>,
    #[serde(default)]
    pub content_md5: Option<[u8; 16]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LegacyS3Ticket {
    pub version: u8,
    pub download_url: String,
    pub object_key: String,
    pub common: TicketCommon,
}

#[cfg(test)]
mod tests {
    use super::*;
    use iroh::{SecretKey, TransportAddr};
    use std::net::{Ipv4Addr, SocketAddr};

    #[test]
    fn peer_ticket_round_trip() {
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
            Some([7; 16]),
        );
        let raw = ticket.encode().unwrap();
        let decoded = Ticket::decode(&raw).unwrap();
        assert_eq!(ticket, decoded);
    }

    #[test]
    fn s3_ticket_round_trip() {
        let ticket = Ticket::s3(
            "https://example.com/file".into(),
            Some("https://example.com/delete".into()),
            "ii/abc-file.txt".into(),
            "file.txt".into(),
            PayloadKind::File,
            Some(12),
            Some([7; 16]),
        );
        let raw = ticket.encode().unwrap();
        let decoded = Ticket::decode(&raw).unwrap();
        assert_eq!(ticket, decoded);
    }

    #[test]
    fn webdav_ticket_round_trip() {
        let ticket = Ticket::webdav(
            "default".into(),
            "ii/abc".into(),
            true,
            Some(WebDavPortableCredentials {
                url: "https://dav.example.com/".into(),
                username: "user".into(),
                password: "pass".into(),
                auth: "basic".into(),
            }),
            "file.txt".into(),
            PayloadKind::File,
            Some(12),
            Some([7; 16]),
        );
        let raw = ticket.encode().unwrap();
        let decoded = Ticket::decode(&raw).unwrap();
        assert_eq!(ticket, decoded);
        match decoded {
            Ticket::WebDav(webdav) => assert!(webdav.delete_after_recv),
            _ => panic!("expected webdav ticket"),
        }
    }

    #[test]
    fn legacy_ticket_decodes() {
        let legacy = LegacyTicket {
            version: 1,
            endpoint: EndpointAddr::from_parts(
                SecretKey::generate().public(),
                [TransportAddr::Ip(SocketAddr::from((
                    Ipv4Addr::LOCALHOST,
                    1234,
                )))],
            ),
            name: "legacy.txt".into(),
            kind: PayloadKind::File,
            size: Some(5),
            content_md5: None,
        };
        let bytes = postcard::to_stdvec(&legacy).unwrap();
        let raw = format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(bytes));
        let decoded = Ticket::decode(&raw).unwrap();
        match decoded {
            Ticket::Peer(peer) => {
                assert_eq!(peer.version, 2);
                assert_eq!(peer.common.name, "legacy.txt");
            }
            _ => panic!("expected peer ticket"),
        }
    }

    #[test]
    fn legacy_s3_ticket_decodes_without_delete_url() {
        let legacy = LegacyS3Ticket {
            version: 3,
            download_url: "https://example.com/file".into(),
            object_key: "ii/abc".into(),
            common: TicketCommon {
                name: "file.txt".into(),
                kind: PayloadKind::File,
                size: Some(12),
                content_md5: Some([1; 16]),
            },
        };
        let bytes = postcard::to_stdvec(&legacy).unwrap();
        let raw = format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(bytes));
        let decoded = Ticket::decode(&raw).unwrap();
        match decoded {
            Ticket::S3(s3) => {
                assert_eq!(s3.delete_url, None);
                assert_eq!(s3.object_key, "ii/abc");
            }
            _ => panic!("expected s3 ticket"),
        }
    }
}
