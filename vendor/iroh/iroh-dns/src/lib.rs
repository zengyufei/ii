//! DNS-based endpoint discovery for iroh.
//!
//! This crate contains the core types for publishing and resolving iroh endpoint
//! information via DNS, using the [pkarr](https://pkarr.org) signed packet format.
#![deny(missing_docs, rustdoc::broken_intra_doc_links, unreachable_pub)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]

#[cfg(feature = "peer-discovery")]
mod attrs;
#[cfg(not(wasm_browser))]
pub mod dns;
#[cfg(feature = "peer-discovery")]
pub mod endpoint_info;
#[cfg(feature = "peer-discovery")]
pub mod pkarr;

#[cfg(feature = "peer-discovery")]
pub use attrs::{EncodingError, IROH_TXT_NAME, ParseError};
