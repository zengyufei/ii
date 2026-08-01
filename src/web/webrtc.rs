use crate::web::http::{
    local_web_host, read_web_request, web_token_path, write_web_error, write_web_response,
    write_web_response_with_headers,
};
use anyhow::{Context, Result, bail};
use std::{
    collections::{BTreeMap, VecDeque},
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::{io::AsyncReadExt, net::TcpStream};

pub(crate) struct WebRtcServer {
    pub(crate) state: Mutex<WebRtcState>,
}

pub(crate) struct WebRtcState {
    pub(crate) next_peer_id: u64,
    pub(crate) peers: BTreeMap<u64, WebRtcPeer>,
}

pub(crate) struct WebRtcPeer {
    pub(crate) last_seen: Instant,
    pub(crate) signals: VecDeque<WebRtcSignal>,
}

pub(crate) struct WebRtcSignal {
    pub(crate) from: u64,
    pub(crate) body: Vec<u8>,
}

#[derive(Debug)]
pub(crate) enum WebRtcRelayError {
    MissingPeer,
    QueueFull,
}

pub(crate) const WEBRTC_PEER_TTL: Duration = Duration::from_secs(30);
const WEBRTC_MAX_SIGNAL_BYTES: u64 = 128 * 1024;
pub(crate) const WEBRTC_MAX_PENDING_SIGNALS: usize = 64;
impl WebRtcServer {
    pub(crate) fn new() -> Self {
        Self {
            state: Mutex::new(WebRtcState {
                next_peer_id: 1,
                peers: BTreeMap::new(),
            }),
        }
    }

    pub(crate) fn join(&self) -> Option<u64> {
        let now = Instant::now();
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        prune_webrtc_peers(&mut state, now);
        let peer_id = state.next_peer_id;
        state.next_peer_id = state.next_peer_id.checked_add(1)?;
        state.peers.insert(
            peer_id,
            WebRtcPeer {
                last_seen: now,
                signals: VecDeque::new(),
            },
        );
        Some(peer_id)
    }

    pub(crate) fn peers(&self, peer_id: u64) -> Option<Vec<u64>> {
        let now = Instant::now();
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        prune_webrtc_peers(&mut state, now);
        state.peers.get_mut(&peer_id)?.last_seen = now;
        Some(
            state
                .peers
                .keys()
                .copied()
                .filter(|id| *id != peer_id)
                .collect(),
        )
    }

    pub(crate) fn heartbeat(&self, peer_id: u64) -> bool {
        let now = Instant::now();
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        prune_webrtc_peers(&mut state, now);
        let Some(peer) = state.peers.get_mut(&peer_id) else {
            return false;
        };
        peer.last_seen = now;
        true
    }

    pub(crate) fn relay(&self, from: u64, to: u64, body: Vec<u8>) -> Result<(), WebRtcRelayError> {
        let now = Instant::now();
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        prune_webrtc_peers(&mut state, now);
        let Some(sender) = state.peers.get_mut(&from) else {
            return Err(WebRtcRelayError::MissingPeer);
        };
        sender.last_seen = now;
        let Some(recipient) = state.peers.get_mut(&to) else {
            return Err(WebRtcRelayError::MissingPeer);
        };
        if recipient.signals.len() >= WEBRTC_MAX_PENDING_SIGNALS {
            return Err(WebRtcRelayError::QueueFull);
        }
        recipient.signals.push_back(WebRtcSignal { from, body });
        Ok(())
    }

    pub(crate) fn poll(&self, peer_id: u64) -> Option<Option<WebRtcSignal>> {
        let now = Instant::now();
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        prune_webrtc_peers(&mut state, now);
        let peer = state.peers.get_mut(&peer_id)?;
        peer.last_seen = now;
        Some(peer.signals.pop_front())
    }
}

fn prune_webrtc_peers(state: &mut WebRtcState, now: Instant) {
    state
        .peers
        .retain(|_, peer| now.duration_since(peer.last_seen) < WEBRTC_PEER_TTL);
}
pub(crate) async fn serve_connection(
    mut stream: TcpStream,
    server: Arc<WebRtcServer>,
    web_token: Option<String>,
) -> Result<()> {
    let client_ip = match stream.peer_addr().ok() {
        Some(SocketAddr::V4(address)) if address.ip().is_loopback() => local_web_host().to_string(),
        Some(SocketAddr::V4(address)) => address.ip().to_string(),
        _ => String::new(),
    };
    let mut request = match read_web_request(&mut stream).await {
        Ok(request) => request,
        Err(err) => {
            let message = format!("bad request: {err}");
            return write_web_error(&mut stream, "400 Bad Request", &message).await;
        }
    };
    let Some(path) = web_token_path(web_token.as_deref(), &request.target) else {
        return write_web_error(&mut stream, "404 Not Found", "not found").await;
    };

    match request.method.as_str() {
        "GET" if path.is_empty() => {
            let page = WEBRTC_PAGE.replace("__II_CLIENT_IP__", &client_ip);
            write_web_response_with_headers(
                &mut stream,
                "200 OK",
                "text/html; charset=utf-8",
                "Cache-Control: no-store\r\n",
                page.as_bytes(),
            )
            .await
        }
        "POST" if path == "join" => match server.join() {
            Some(peer_id) => {
                write_web_response(
                    &mut stream,
                    "201 Created",
                    "text/plain; charset=utf-8",
                    peer_id.to_string().as_bytes(),
                )
                .await
            }
            None => {
                write_web_error(&mut stream, "503 Service Unavailable", "peer limit reached").await
            }
        },
        "GET" => {
            if let Some(peer_id) = webrtc_single_peer_path(path, "peers") {
                let Some(peers) = server.peers(peer_id) else {
                    return write_web_error(&mut stream, "404 Not Found", "peer not found").await;
                };
                let mut body = String::from("[");
                for (index, peer_id) in peers.iter().enumerate() {
                    if index > 0 {
                        body.push(',');
                    }
                    body.push_str(&peer_id.to_string());
                }
                body.push(']');
                return write_web_response(
                    &mut stream,
                    "200 OK",
                    "application/json; charset=utf-8",
                    body.as_bytes(),
                )
                .await;
            }
            if let Some(peer_id) = webrtc_single_peer_path(path, "signal") {
                return match server.poll(peer_id) {
                    None => write_web_error(&mut stream, "404 Not Found", "peer not found").await,
                    Some(None) => {
                        write_web_response(
                            &mut stream,
                            "204 No Content",
                            "text/plain; charset=utf-8",
                            b"",
                        )
                        .await
                    }
                    Some(Some(signal)) => {
                        write_web_response_with_headers(
                            &mut stream,
                            "200 OK",
                            "application/json; charset=utf-8",
                            &format!("X-II-From: {}\r\n", signal.from),
                            &signal.body,
                        )
                        .await
                    }
                };
            }
            write_web_error(&mut stream, "404 Not Found", "not found").await
        }
        "POST" if webrtc_single_peer_path(path, "heartbeat").is_some() => {
            let peer_id = webrtc_single_peer_path(path, "heartbeat").unwrap();
            if server.heartbeat(peer_id) {
                write_web_response(
                    &mut stream,
                    "204 No Content",
                    "text/plain; charset=utf-8",
                    b"",
                )
                .await
            } else {
                write_web_error(&mut stream, "404 Not Found", "peer not found").await
            }
        }
        "POST" if webrtc_signal_path(path).is_some() => {
            let (from, to) = webrtc_signal_path(path).unwrap();
            let Some(content_length) = request.content_length else {
                return write_web_error(
                    &mut stream,
                    "411 Length Required",
                    "Content-Length is required",
                )
                .await;
            };
            if content_length == 0 {
                return write_web_error(&mut stream, "400 Bad Request", "signal body is missing")
                    .await;
            }
            if content_length > WEBRTC_MAX_SIGNAL_BYTES {
                return write_web_error(
                    &mut stream,
                    "413 Payload Too Large",
                    "signal body is too large",
                )
                .await;
            }
            let body = match read_webrtc_signal_body(
                &mut stream,
                content_length,
                std::mem::take(&mut request.body),
            )
            .await
            {
                Ok(body) if std::str::from_utf8(&body).is_ok() => body,
                Ok(_) => {
                    return write_web_error(
                        &mut stream,
                        "400 Bad Request",
                        "signal body is not UTF-8",
                    )
                    .await;
                }
                Err(err) => {
                    let message = format!("invalid signal body: {err}");
                    return write_web_error(&mut stream, "400 Bad Request", &message).await;
                }
            };
            match server.relay(from, to, body) {
                Ok(()) => {
                    write_web_response(
                        &mut stream,
                        "204 No Content",
                        "text/plain; charset=utf-8",
                        b"",
                    )
                    .await
                }
                Err(WebRtcRelayError::MissingPeer) => {
                    write_web_error(&mut stream, "404 Not Found", "peer not found").await
                }
                Err(WebRtcRelayError::QueueFull) => {
                    write_web_error(&mut stream, "429 Too Many Requests", "signal queue is full")
                        .await
                }
            }
        }
        "POST" => write_web_error(&mut stream, "404 Not Found", "not found").await,
        _ => write_web_error(&mut stream, "405 Method Not Allowed", "method not allowed").await,
    }
}

fn webrtc_single_peer_path(path: &str, endpoint: &str) -> Option<u64> {
    let query = path.strip_prefix(endpoint)?.strip_prefix("?id=")?;
    parse_webrtc_peer_id(query)
}

fn webrtc_signal_path(path: &str) -> Option<(u64, u64)> {
    let query = path.strip_prefix("signal?")?;
    let mut from = None;
    let mut to = None;
    for part in query.split('&') {
        let (key, value) = part.split_once('=')?;
        match key {
            "from" if from.replace(parse_webrtc_peer_id(value)?).is_none() => {}
            "to" if to.replace(parse_webrtc_peer_id(value)?).is_none() => {}
            _ => return None,
        }
    }
    let (Some(from), Some(to)) = (from, to) else {
        return None;
    };
    (from != to).then_some((from, to))
}

fn parse_webrtc_peer_id(value: &str) -> Option<u64> {
    let peer_id = value.parse().ok()?;
    (peer_id != 0).then_some(peer_id)
}

async fn read_webrtc_signal_body(
    stream: &mut TcpStream,
    content_length: u64,
    mut body: Vec<u8>,
) -> Result<Vec<u8>> {
    let initial_length = u64::try_from(body.len()).context("signal body is too large")?;
    if initial_length > content_length {
        bail!("signal body exceeds Content-Length");
    }
    let remaining = content_length - initial_length;
    let mut body_reader = stream.take(remaining);
    let copied = body_reader
        .read_to_end(&mut body)
        .await
        .context("read signal body")?;
    if u64::try_from(copied).context("signal body is too large")? != remaining {
        bail!("signal body ended early");
    }
    Ok(body)
}

const WEBRTC_PAGE: &str = include_str!("../webrtc_page.html");
