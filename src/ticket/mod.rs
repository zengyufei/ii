use anyhow::Result;
use iroh::EndpointAddr;

mod codec;
mod legacy;
mod model;

pub use model::*;

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

    pub fn relay_only(
        endpoint: EndpointAddr,
        name: String,
        kind: PayloadKind,
        size: Option<u64>,
        content_md5: Option<[u8; 16]>,
    ) -> Self {
        Ticket::RelayOnly(RelayOnlyTicket {
            version: 5,
            endpoint,
            common: TicketCommon {
                name,
                kind,
                size,
                content_md5,
            },
        })
    }

    pub fn trusted_relay_only(
        endpoint: EndpointAddr,
        name: String,
        kind: PayloadKind,
        size: Option<u64>,
        content_md5: Option<[u8; 16]>,
    ) -> Self {
        Ticket::TrustedRelayOnly(RelayOnlyTicket {
            version: 6,
            endpoint,
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

    pub fn ftp(
        profile: String,
        object_key: String,
        delete_after_recv: bool,
        portable: Option<FtpPortableCredentials>,
        name: String,
        kind: PayloadKind,
        size: Option<u64>,
        content_md5: Option<[u8; 16]>,
    ) -> Self {
        Ticket::Ftp(FtpTicket {
            version: 7,
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

    pub fn sftp(
        profile: String,
        object_key: String,
        delete_after_recv: bool,
        portable: Option<SftpPortableCredentials>,
        name: String,
        kind: PayloadKind,
        size: Option<u64>,
        content_md5: Option<[u8; 16]>,
    ) -> Self {
        Ticket::Sftp(SftpTicket {
            version: 8,
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

    pub fn tunnel(
        endpoint: EndpointAddr,
        access_key: [u8; 32],
        relay_mode: TunnelRelayMode,
    ) -> Self {
        Ticket::Tunnel(TunnelTicket {
            version: 9,
            endpoint,
            access_key,
            relay_mode,
        })
    }

    pub fn encode(&self) -> Result<String> {
        codec::encode(self)
    }

    pub fn decode(raw: &str) -> Result<Self> {
        codec::decode(raw)
    }

    pub fn common(&self) -> &TicketCommon {
        match self {
            Ticket::Peer(ticket) => &ticket.common,
            Ticket::S3(ticket) => &ticket.common,
            Ticket::WebDav(ticket) => &ticket.common,
            Ticket::RelayOnly(ticket) => &ticket.common,
            Ticket::TrustedRelayOnly(ticket) => &ticket.common,
            Ticket::Ftp(ticket) => &ticket.common,
            Ticket::Sftp(ticket) => &ticket.common,
            Ticket::Tunnel(_) => unreachable!("tunnel tickets have no file metadata"),
        }
    }

    pub fn endpoint(&self) -> Option<&EndpointAddr> {
        match self {
            Ticket::Peer(ticket) => Some(&ticket.endpoint),
            Ticket::RelayOnly(ticket) => Some(&ticket.endpoint),
            Ticket::TrustedRelayOnly(ticket) => Some(&ticket.endpoint),
            Ticket::Tunnel(ticket) => Some(&ticket.endpoint),
            Ticket::S3(_) | Ticket::WebDav(_) | Ticket::Ftp(_) | Ticket::Sftp(_) => None,
        }
    }

    pub fn s3_route(&self) -> Option<&S3Ticket> {
        match self {
            Ticket::Peer(_)
            | Ticket::WebDav(_)
            | Ticket::RelayOnly(_)
            | Ticket::TrustedRelayOnly(_)
            | Ticket::Ftp(_)
            | Ticket::Sftp(_)
            | Ticket::Tunnel(_) => None,
            Ticket::S3(ticket) => Some(ticket),
        }
    }

    pub fn webdav_route(&self) -> Option<&WebDavTicket> {
        match self {
            Ticket::Peer(_)
            | Ticket::S3(_)
            | Ticket::RelayOnly(_)
            | Ticket::TrustedRelayOnly(_)
            | Ticket::Ftp(_)
            | Ticket::Sftp(_)
            | Ticket::Tunnel(_) => None,
            Ticket::WebDav(ticket) => Some(ticket),
        }
    }

    pub fn ftp_route(&self) -> Option<&FtpTicket> {
        match self {
            Ticket::Ftp(ticket) => Some(ticket),
            Ticket::Peer(_)
            | Ticket::S3(_)
            | Ticket::WebDav(_)
            | Ticket::RelayOnly(_)
            | Ticket::TrustedRelayOnly(_)
            | Ticket::Sftp(_)
            | Ticket::Tunnel(_) => None,
        }
    }

    pub fn sftp_route(&self) -> Option<&SftpTicket> {
        match self {
            Ticket::Sftp(ticket) => Some(ticket),
            Ticket::Peer(_)
            | Ticket::S3(_)
            | Ticket::WebDav(_)
            | Ticket::RelayOnly(_)
            | Ticket::TrustedRelayOnly(_)
            | Ticket::Ftp(_)
            | Ticket::Tunnel(_) => None,
        }
    }

    pub fn is_relay_only(&self) -> bool {
        matches!(self, Ticket::RelayOnly(_) | Ticket::TrustedRelayOnly(_))
    }

    pub fn is_self_signed_relay_only(&self) -> bool {
        matches!(self, Ticket::RelayOnly(_))
    }

    pub fn tunnel_route(&self) -> Option<&TunnelTicket> {
        match self {
            Ticket::Tunnel(ticket) => Some(ticket),
            Ticket::Peer(_)
            | Ticket::S3(_)
            | Ticket::WebDav(_)
            | Ticket::RelayOnly(_)
            | Ticket::TrustedRelayOnly(_)
            | Ticket::Ftp(_)
            | Ticket::Sftp(_) => None,
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
    fn relay_only_ticket_round_trip_contains_no_ip_transport() {
        let relay: iroh::RelayUrl = "https://127.0.0.1:8443".parse().unwrap();
        let ticket = Ticket::relay_only(
            EndpointAddr::from_parts(
                SecretKey::generate().public(),
                [TransportAddr::Relay(relay)],
            ),
            "hello.txt".into(),
            PayloadKind::File,
            Some(12),
            Some([7; 16]),
        );
        let raw = ticket.encode().unwrap();
        let decoded = Ticket::decode(&raw).unwrap();
        assert!(decoded.is_self_signed_relay_only());
        let endpoint = decoded.endpoint().unwrap();
        assert_eq!(endpoint.ip_addrs().count(), 0);
        assert_eq!(endpoint.relay_urls().count(), 1);
    }

    #[test]
    fn trusted_relay_only_ticket_round_trip_keeps_tls_mode() {
        let relay: iroh::RelayUrl = "https://relay.example.com".parse().unwrap();
        let ticket = Ticket::trusted_relay_only(
            EndpointAddr::from_parts(
                SecretKey::generate().public(),
                [TransportAddr::Relay(relay)],
            ),
            "hello.txt".into(),
            PayloadKind::File,
            Some(12),
            Some([7; 16]),
        );
        let raw = ticket.encode().unwrap();
        let decoded = Ticket::decode(&raw).unwrap();
        assert!(decoded.is_relay_only());
        assert!(!decoded.is_self_signed_relay_only());
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
    fn ftp_ticket_round_trip() {
        let ticket = Ticket::ftp(
            "legacy".into(),
            "ii/abc".into(),
            true,
            Some(FtpPortableCredentials {
                url: "ftp://ftp.example.com".into(),
                username: "user".into(),
                password: "pass".into(),
                remote_dir: "ii/".into(),
            }),
            "file.txt".into(),
            PayloadKind::File,
            Some(12),
            Some([7; 16]),
        );
        let decoded = Ticket::decode(&ticket.encode().unwrap()).unwrap();
        assert_eq!(ticket, decoded);
        assert!(decoded.ftp_route().unwrap().delete_after_recv);
    }

    #[test]
    fn sftp_password_and_private_key_tickets_round_trip() {
        let password = Ticket::sftp(
            "server".into(),
            "ii/password".into(),
            false,
            Some(SftpPortableCredentials {
                host: "sftp.example.com".into(),
                port: 22,
                username: "user".into(),
                remote_dir: "ii/".into(),
                auth: SftpPortableAuth::Password {
                    password: "pass".into(),
                },
            }),
            "file.txt".into(),
            PayloadKind::File,
            Some(12),
            Some([7; 16]),
        );
        let private_key = Ticket::sftp(
            "server".into(),
            "ii/key".into(),
            true,
            Some(SftpPortableCredentials {
                host: "sftp.example.com".into(),
                port: 2222,
                username: "user".into(),
                remote_dir: "ii/".into(),
                auth: SftpPortableAuth::PrivateKey {
                    private_key: "-----BEGIN OPENSSH PRIVATE KEY-----".into(),
                    private_key_passphrase: Some("passphrase".into()),
                },
            }),
            "file.txt".into(),
            PayloadKind::File,
            Some(12),
            Some([7; 16]),
        );

        assert_eq!(
            password,
            Ticket::decode(&password.encode().unwrap()).unwrap()
        );
        assert_eq!(
            private_key,
            Ticket::decode(&private_key.encode().unwrap()).unwrap()
        );
    }

    #[test]
    fn tunnel_ticket_round_trip_preserves_access_key_and_relay_mode() {
        let endpoint = EndpointAddr::from_parts(
            SecretKey::generate().public(),
            [TransportAddr::Relay(
                "https://relay.example.com:8443".parse().unwrap(),
            )],
        );
        let ticket = Ticket::tunnel(endpoint, [9; 32], TunnelRelayMode::SelfSignedRelayOnly);
        let decoded = Ticket::decode(&ticket.encode().unwrap()).unwrap();
        assert_eq!(decoded, ticket);
        assert!(decoded.tunnel_route().is_some());
    }

    #[test]
    fn legacy_ticket_decodes() {
        let legacy = legacy::LegacyTicket {
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
        let raw = codec::prefixed(bytes);
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
        let legacy = legacy::LegacyS3Ticket {
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
        let raw = codec::prefixed(bytes);
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
