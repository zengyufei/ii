use super::model::{PayloadKind, TicketCommon};
use iroh::EndpointAddr;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct LegacyTicket {
    pub version: u8,
    pub endpoint: EndpointAddr,
    pub name: String,
    pub kind: PayloadKind,
    pub size: Option<u64>,
    #[serde(default)]
    pub content_md5: Option<[u8; 16]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct LegacyS3Ticket {
    pub version: u8,
    pub download_url: String,
    pub object_key: String,
    pub common: TicketCommon,
}
