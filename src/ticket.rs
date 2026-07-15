use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use iroh::EndpointAddr;
use serde::{Deserialize, Serialize};

const PREFIX: &str = "ii1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Ticket {
    pub version: u8,
    pub endpoint: EndpointAddr,
    pub name: String,
    pub kind: PayloadKind,
    pub size: Option<u64>,
    pub content_md5: Option<[u8; 16]>,
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
            if ticket.version != 2 {
                bail!("unsupported ticket version {}", ticket.version);
            }
            return Ok(ticket);
        }
        let legacy: LegacyTicket = postcard::from_bytes(&bytes).context("parse ticket")?;
        if legacy.version != 1 {
            bail!("unsupported ticket version {}", legacy.version);
        }
        Ok(Ticket {
            version: 1,
            endpoint: legacy.endpoint,
            name: legacy.name,
            kind: legacy.kind,
            size: legacy.size,
            content_md5: None,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LegacyTicket {
    pub version: u8,
    pub endpoint: EndpointAddr,
    pub name: String,
    pub kind: PayloadKind,
    pub size: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use iroh::{SecretKey, TransportAddr};
    use std::net::{Ipv4Addr, SocketAddr};

    #[test]
    fn ticket_v2_round_trip() {
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
            content_md5: Some([7; 16]),
        };
        let raw = ticket.encode().unwrap();
        let decoded = Ticket::decode(&raw).unwrap();
        assert_eq!(ticket, decoded);
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
        };
        let bytes = postcard::to_stdvec(&legacy).unwrap();
        let raw = format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(bytes));
        let decoded = Ticket::decode(&raw).unwrap();
        assert_eq!(decoded.version, 1);
        assert_eq!(decoded.content_md5, None);
        assert_eq!(decoded.name, "legacy.txt");
    }
}
