use super::{legacy, model::*};
use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

const PREFIX: &str = "ii1";

pub(super) fn encode(ticket: &Ticket) -> Result<String> {
    let bytes = postcard::to_stdvec(ticket).context("serialize ticket")?;
    Ok(format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(bytes)))
}

pub(super) fn decode(raw: &str) -> Result<Ticket> {
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
            Ticket::RelayOnly(relay) if relay.version == 5 => return Ok(ticket),
            Ticket::TrustedRelayOnly(relay) if relay.version == 6 => return Ok(ticket),
            Ticket::Ftp(ftp) if ftp.version == 7 => return Ok(ticket),
            Ticket::Sftp(sftp) if sftp.version == 8 => return Ok(ticket),
            Ticket::Tunnel(tunnel) if tunnel.version == 9 => return Ok(ticket),
            Ticket::Peer(peer) if peer.version != 2 => {
                bail!("unsupported peer ticket version {}", peer.version)
            }
            Ticket::S3(s3) if s3.version != 3 => {
                bail!("unsupported s3 ticket version {}", s3.version)
            }
            Ticket::WebDav(webdav) if webdav.version != 4 => {
                bail!("unsupported webdav ticket version {}", webdav.version)
            }
            Ticket::RelayOnly(relay) if relay.version != 5 => {
                bail!("unsupported relay-only ticket version {}", relay.version)
            }
            Ticket::TrustedRelayOnly(relay) if relay.version != 6 => {
                bail!(
                    "unsupported trusted relay-only ticket version {}",
                    relay.version
                )
            }
            Ticket::Ftp(ftp) if ftp.version != 7 => {
                bail!("unsupported ftp ticket version {}", ftp.version)
            }
            Ticket::Sftp(sftp) if sftp.version != 8 => {
                bail!("unsupported sftp ticket version {}", sftp.version)
            }
            Ticket::Tunnel(tunnel) if tunnel.version != 9 => {
                bail!("unsupported tunnel ticket version {}", tunnel.version)
            }
            _ => {}
        }
    }
    if let Ok(legacy_s3) = postcard::from_bytes::<legacy::LegacyS3Ticket>(&bytes) {
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
    let legacy: legacy::LegacyTicket = postcard::from_bytes(&bytes).context("parse ticket")?;
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

#[cfg(test)]
pub(super) fn prefixed(bytes: Vec<u8>) -> String {
    format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(bytes))
}
