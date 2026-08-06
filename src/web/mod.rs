pub(crate) mod dav;
pub(crate) mod directory;
pub(crate) mod http;
pub(crate) mod qr;
pub(crate) mod upload;
pub(crate) mod webrtc;

pub(crate) use dav::serve_dav;
pub(crate) use directory::directory_root;
pub(crate) use http::{
    WebContent, WebServeLifetime, lan_ipv4_hosts, serve_web, start_lan_web_server, web_upload_dir,
};
pub(crate) use webrtc::{WebRtcServer, serve_connection as serve_webrtc_connection};

#[cfg(test)]
mod directory_tests;
#[cfg(test)]
mod http_tests;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod upload_tests;
#[cfg(test)]
mod webrtc_tests;
