//! Stable GUI-facing transfer API.
//!
//! Command orchestration lives in `crate::service`; this module keeps the
//! established GUI import path intact.

pub use crate::service::{
    TransferEvent, recv, recv_with_events, send, send_with_events, tunnel, web, webrtc,
};
