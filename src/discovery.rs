use anyhow::{Context, Result};
use rand::RngExt;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    net::{Ipv6Addr, SocketAddr, SocketAddrV6},
};
use tokio::{
    net::UdpSocket,
    task::JoinHandle,
    time::{self, Duration},
};

const DISCOVERY_PORT: u16 = 43_917;
const MAX_PACKET: usize = 16 * 1024;
const MAGIC: &[u8; 4] = b"IID1";
const ALL_NODES_V6: Ipv6Addr = Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 0, 1);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) enum Service {
    Send {
        ticket: String,
        name: String,
        kind: String,
        size: Option<u64>,
    },
    Web {
        url: String,
    },
    Dav {
        url: String,
    },
    Http {
        kind: String,
        url: String,
    },
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
enum Packet {
    Announce {
        id: u64,
    },
    Query {
        id: u64,
        nonce: u64,
    },
    Response {
        id: u64,
        nonce: u64,
        service: Service,
    },
}

pub(crate) struct Advertiser {
    tasks: Vec<JoinHandle<()>>,
}

impl Drop for Advertiser {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

pub(crate) async fn advertise(service: Service) -> Result<Advertiser> {
    let id = rand::rng().random::<u64>();
    let mut tasks = vec![spawn_ipv4_advertiser(service.clone(), id).await?];
    if let Ok(task) = spawn_ipv6_advertiser(service, id).await {
        tasks.push(task);
    }
    Ok(Advertiser { tasks })
}

async fn spawn_ipv4_advertiser(service: Service, id: u64) -> Result<JoinHandle<()>> {
    let socket = UdpSocket::bind("0.0.0.0:0")
        .await
        .context("bind IPv4 LAN discovery socket")?;
    socket
        .set_broadcast(true)
        .context("enable IPv4 LAN discovery broadcast")?;
    Ok(spawn_advertiser(socket, service, id, move |socket| {
        Box::pin(async move {
            let _ = socket
                .send_to(
                    &encode(&Packet::Announce { id }),
                    ("255.255.255.255", DISCOVERY_PORT),
                )
                .await;
        })
    }))
}

async fn spawn_ipv6_advertiser(service: Service, id: u64) -> Result<JoinHandle<()>> {
    let socket = UdpSocket::bind((Ipv6Addr::UNSPECIFIED, 0))
        .await
        .context("bind IPv6 LAN discovery socket")?;
    socket
        .set_multicast_loop_v6(true)
        .context("enable IPv6 LAN discovery multicast loopback")?;
    let interfaces = ipv6_interface_indices();
    Ok(spawn_advertiser(socket, service, id, move |socket| {
        let interfaces = interfaces.clone();
        Box::pin(async move {
            let packet = encode(&Packet::Announce { id });
            for interface in &interfaces {
                let destination = SocketAddrV6::new(ALL_NODES_V6, DISCOVERY_PORT, 0, *interface);
                let _ = socket.send_to(&packet, destination).await;
            }
        })
    }))
}

fn spawn_advertiser<F>(socket: UdpSocket, service: Service, id: u64, announce: F) -> JoinHandle<()>
where
    F: Fn(&UdpSocket) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send + '_>> + Send + 'static,
{
    tokio::spawn(async move {
        let mut ticker = time::interval(Duration::from_secs(1));
        ticker.tick().await;
        let mut buf = [0u8; MAX_PACKET];
        loop {
            tokio::select! {
                _ = ticker.tick() => announce(&socket).await,
                received = socket.recv_from(&mut buf) => {
                    let Ok((len, peer)) = received else { break };
                    let Some(Packet::Query { id: query_id, nonce }) = decode(&buf[..len]) else { continue };
                    if query_id != id { continue; }
                    let _ = socket.send_to(&encode(&Packet::Response { id, nonce, service: service.clone() }), peer).await;
                }
            }
        }
    })
}

pub(crate) async fn discover() -> Result<Vec<Service>> {
    let ipv4 = UdpSocket::bind(("0.0.0.0", DISCOVERY_PORT))
        .await
        .context("listen for IPv4 LAN discovery")?;
    let ipv6 = bind_ipv6_discovery_socket().await.ok();
    let deadline = time::Instant::now() + Duration::from_secs(3);
    let mut pending = BTreeMap::<(SocketAddr, u64), u64>::new();
    let mut services = BTreeMap::<u64, Service>::new();
    let mut ipv4_buf = [0u8; MAX_PACKET];
    let mut ipv6_buf = [0u8; MAX_PACKET];

    loop {
        tokio::select! {
            _ = time::sleep_until(deadline) => break,
            received = ipv4.recv_from(&mut ipv4_buf) => {
                let Ok((len, peer)) = received else { break };
                handle_packet(&ipv4, &ipv4_buf[..len], peer, &mut pending, &mut services).await;
            }
            received = async {
                match &ipv6 {
                    Some(socket) => socket.recv_from(&mut ipv6_buf).await,
                    None => std::future::pending().await,
                }
            } => {
                let Ok((len, peer)) = received else { continue };
                if let Some(socket) = &ipv6 {
                    handle_packet(socket, &ipv6_buf[..len], peer, &mut pending, &mut services).await;
                }
            }
        }
    }
    Ok(services.into_values().collect())
}

async fn bind_ipv6_discovery_socket() -> Result<UdpSocket> {
    let socket = UdpSocket::bind((Ipv6Addr::UNSPECIFIED, DISCOVERY_PORT))
        .await
        .context("listen for IPv6 LAN discovery")?;
    socket
        .set_multicast_loop_v6(true)
        .context("enable IPv6 LAN discovery multicast loopback")?;
    for interface in ipv6_interface_indices() {
        let _ = socket.join_multicast_v6(&ALL_NODES_V6, interface);
    }
    Ok(socket)
}

async fn handle_packet(
    socket: &UdpSocket,
    bytes: &[u8],
    peer: SocketAddr,
    pending: &mut BTreeMap<(SocketAddr, u64), u64>,
    services: &mut BTreeMap<u64, Service>,
) {
    match decode(bytes) {
        Some(Packet::Announce { id }) => {
            let nonce = rand::rng().random::<u64>();
            pending.insert((peer, id), nonce);
            let _ = socket
                .send_to(&encode(&Packet::Query { id, nonce }), peer)
                .await;
        }
        Some(Packet::Response { id, nonce, service })
            if pending.get(&(peer, id)) == Some(&nonce) =>
        {
            services.insert(id, service);
        }
        _ => {}
    }
}

fn encode(packet: &Packet) -> Vec<u8> {
    let mut bytes = Vec::from(MAGIC.as_slice());
    if let Ok(payload) = postcard::to_stdvec(packet) {
        bytes.extend_from_slice(&payload);
    }
    bytes
}

fn decode(bytes: &[u8]) -> Option<Packet> {
    (bytes.len() <= MAX_PACKET)
        .then_some(bytes)?
        .strip_prefix(MAGIC)
        .and_then(|payload| postcard::from_bytes(payload).ok())
}

#[cfg(unix)]
fn ipv6_interface_indices() -> Vec<u32> {
    use std::{
        ffi::{c_char, c_int, c_void},
        ptr,
    };

    #[repr(C)]
    struct IfAddrs {
        next: *mut IfAddrs,
        name: *mut c_char,
        flags: u32,
        address: *mut c_void,
        netmask: *mut c_void,
        destination: *mut c_void,
        data: *mut c_void,
    }

    unsafe extern "C" {
        fn getifaddrs(addresses: *mut *mut IfAddrs) -> c_int;
        fn freeifaddrs(addresses: *mut IfAddrs);
        fn if_nametoindex(name: *const c_char) -> u32;
    }

    let mut first = ptr::null_mut();
    if unsafe { getifaddrs(&mut first) } != 0 {
        return vec![0];
    }
    let mut indices = Vec::new();
    let mut current = first;
    while !current.is_null() {
        let name = unsafe { (*current).name };
        if !name.is_null() {
            let index = unsafe { if_nametoindex(name) };
            if index != 0 {
                indices.push(index);
            }
        }
        current = unsafe { (*current).next };
    }
    unsafe { freeifaddrs(first) };
    indices.sort_unstable();
    indices.dedup();
    if indices.is_empty() { vec![0] } else { indices }
}

#[cfg(not(unix))]
fn ipv6_interface_indices() -> Vec<u32> {
    // Windows accepts zero as the default IPv6 multicast interface. Keeping this
    // fallback avoids a platform-specific adapter dependency in the CLI binary.
    vec![0]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packets_are_versioned_bounded_and_round_trip() {
        let packet = Packet::Response {
            id: 7,
            nonce: 11,
            service: Service::Web {
                url: "http://192.168.1.2:5000/".to_string(),
            },
        };
        let bytes = encode(&packet);
        assert_eq!(decode(&bytes), Some(packet));
        assert!(decode(b"IID0").is_none());
        assert!(decode(&vec![0; MAX_PACKET + 1]).is_none());
    }

    #[test]
    fn http_service_round_trips() {
        let packet = Packet::Response {
            id: 9,
            nonce: 13,
            service: Service::Http {
                kind: "paste".to_string(),
                url: "http://192.168.1.2:5001/".to_string(),
            },
        };
        assert_eq!(decode(&encode(&packet)), Some(packet));
    }
}
