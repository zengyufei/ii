use crate::{
    command::TunnelArgs,
    ticket::{Ticket, TunnelRelayMode},
    transport::{
        iroh::{EndpointPolicy, TUNNEL_ALPN, bind_endpoint},
        p2p::{RecvTrace, connect_to_peer, relay_only_addr},
    },
};
use anyhow::{Context, Result, bail};
use iroh::{Endpoint, RelayMode, TransportAddr};
use rand::RngExt;
use std::{
    net::{Ipv4Addr, SocketAddr},
    sync::Arc,
};
use tokio::{
    io::{self, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Semaphore,
};

const TUNNEL_AUTH_PREFIX: &[u8; 5] = b"IITN\x01";
const TUNNEL_AUTH_LEN: usize = TUNNEL_AUTH_PREFIX.len() + 32;
const TUNNEL_STATUS_OK: u8 = 1;
const TUNNEL_STATUS_REJECTED: u8 = 2;
const TUNNEL_STATUS_BUSY: u8 = 3;
const TUNNEL_MAX_CONNECTIONS: usize = 64;

pub(super) async fn run(args: TunnelArgs) -> Result<()> {
    run_impl(args).await
}

async fn run_impl(args: TunnelArgs) -> Result<()> {
    match args {
        TunnelArgs::Serve {
            target,
            relay,
            accept_self_signed_relay,
        } => serve_tunnel(target, relay, accept_self_signed_relay).await,
        TunnelArgs::Connect { ticket, listen } => connect_tunnel(ticket, listen).await,
    }
}
async fn serve_tunnel(
    target: String,
    relay: Option<iroh::RelayUrl>,
    accept_self_signed_relay: bool,
) -> Result<()> {
    let policy = match &relay {
        Some(url) if accept_self_signed_relay => EndpointPolicy::SelfSignedRelayOnly(url.clone()),
        Some(url) => EndpointPolicy::TrustedRelayOnly(url.clone()),
        None => EndpointPolicy::standard(RelayMode::Default),
    };
    let endpoint = bind_endpoint(policy, TUNNEL_ALPN, None).await?;
    endpoint.online().await;

    let endpoint_addr = endpoint.addr();
    let (ticket_endpoint, relay_mode) = match &relay {
        Some(url) if accept_self_signed_relay => (
            iroh::EndpointAddr::from_parts(endpoint_addr.id, [TransportAddr::Relay(url.clone())]),
            TunnelRelayMode::SelfSignedRelayOnly,
        ),
        Some(url) => (
            iroh::EndpointAddr::from_parts(endpoint_addr.id, [TransportAddr::Relay(url.clone())]),
            TunnelRelayMode::TrustedRelayOnly,
        ),
        None => (endpoint_addr, TunnelRelayMode::Default),
    };
    let mut access_key = [0u8; 32];
    rand::rng().fill(&mut access_key);
    let ticket = Ticket::tunnel(ticket_endpoint, access_key, relay_mode).encode()?;
    println!("ii tunnel ticket:");
    println!("{ticket}");
    println!();
    println!("on the other computer:");
    println!("ii tunnel -c {ticket}");
    println!();
    println!("forwarding to {target}; press Ctrl+C to stop");

    let permits = Arc::new(Semaphore::new(TUNNEL_MAX_CONNECTIONS));
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            incoming = endpoint.accept() => {
                let Some(incoming) = incoming else {
                    break;
                };
                let target = target.clone();
                let permits = Arc::clone(&permits);
                tokio::spawn(async move {
                    let conn = match incoming.accept() {
                        Ok(conn) => match conn.await {
                            Ok(conn) => conn,
                            Err(err) => {
                                eprintln!("ii tunnel: failed to accept connection: {err:#}");
                                return;
                            }
                        },
                        Err(err) => {
                            eprintln!("ii tunnel: dropped incoming connection: {err:#}");
                            return;
                        }
                    };
                    if let Err(err) = serve_tunnel_connection(conn, target, access_key, permits).await {
                        eprintln!("ii tunnel: connection failed: {err:#}");
                    }
                });
            }
        }
    }
    endpoint.close().await;
    Ok(())
}

async fn serve_tunnel_connection(
    conn: iroh::endpoint::Connection,
    target: String,
    access_key: [u8; 32],
    permits: Arc<Semaphore>,
) -> Result<()> {
    loop {
        let (send, recv) = match conn.accept_bi().await {
            Ok(streams) => streams,
            Err(_) => return Ok(()),
        };
        let target = target.clone();
        let permits = Arc::clone(&permits);
        tokio::spawn(async move {
            if let Err(err) = serve_tunnel_stream(send, recv, target, access_key, permits).await {
                eprintln!("ii tunnel: stream failed: {err:#}");
            }
        });
    }
}

pub(super) async fn serve_tunnel_stream(
    mut send: iroh::endpoint::SendStream,
    mut recv: iroh::endpoint::RecvStream,
    target: String,
    access_key: [u8; 32],
    permits: Arc<Semaphore>,
) -> Result<()> {
    let mut auth = [0u8; TUNNEL_AUTH_LEN];
    if recv.read_exact(&mut auth).await.is_err()
        || auth[..TUNNEL_AUTH_PREFIX.len()] != TUNNEL_AUTH_PREFIX[..]
        || auth[TUNNEL_AUTH_PREFIX.len()..] != access_key
    {
        reject_tunnel_stream(&mut send, TUNNEL_STATUS_REJECTED).await;
        return Ok(());
    }

    let permit = match permits.try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => {
            reject_tunnel_stream(&mut send, TUNNEL_STATUS_BUSY).await;
            return Ok(());
        }
    };
    let target_stream = match TcpStream::connect(&target).await {
        Ok(stream) => stream,
        Err(err) => {
            reject_tunnel_stream(&mut send, TUNNEL_STATUS_REJECTED).await;
            return Err(err).with_context(|| format!("connect tunnel target {target}"));
        }
    };
    send.write_all(&[TUNNEL_STATUS_OK])
        .await
        .context("confirm tunnel stream")?;
    let _permit = permit;
    forward_tunnel_tcp(target_stream, send, recv).await
}

async fn reject_tunnel_stream(send: &mut iroh::endpoint::SendStream, status: u8) {
    let _ = send.write_all(&[status]).await;
    let _ = send.finish();
}

async fn connect_tunnel(ticket_raw: String, listen: Option<SocketAddr>) -> Result<()> {
    let ticket = Ticket::decode(&ticket_raw)?;
    let tunnel = ticket
        .tunnel_route()
        .cloned()
        .context("ticket is not an ii tunnel ticket; use ii recv for file tickets")?;
    let (policy, relay_only) = tunnel_endpoint_policy(&tunnel)?;
    let endpoint = bind_endpoint(policy, TUNNEL_ALPN, None).await?;
    endpoint.online().await;
    let listener = bind_tunnel_listener(listen).await?;
    let listen_addr = listener
        .local_addr()
        .context("read tunnel listen address")?;
    println!("ii tunnel: listening on {listen_addr}; press Ctrl+C to stop");

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            accepted = listener.accept() => {
                let (local, _) = accepted.context("accept local tunnel connection")?;
                let endpoint = endpoint.clone();
                let endpoint_addr = tunnel.endpoint.clone();
                let access_key = tunnel.access_key;
                tokio::spawn(async move {
                    if let Err(err) = connect_tunnel_stream(endpoint, endpoint_addr, relay_only, access_key, local).await {
                        eprintln!("ii tunnel: local connection failed: {err:#}");
                    }
                });
            }
        }
    }
    endpoint.close().await;
    Ok(())
}

fn tunnel_endpoint_policy(ticket: &crate::ticket::TunnelTicket) -> Result<(EndpointPolicy, bool)> {
    match ticket.relay_mode {
        TunnelRelayMode::Default => Ok((EndpointPolicy::standard(RelayMode::Default), false)),
        TunnelRelayMode::SelfSignedRelayOnly | TunnelRelayMode::TrustedRelayOnly => {
            let relay_url = ticket
                .endpoint
                .relay_urls()
                .next()
                .cloned()
                .context("relay-only tunnel ticket is missing its relay URL")?;
            let policy = if ticket.relay_mode == TunnelRelayMode::SelfSignedRelayOnly {
                EndpointPolicy::SelfSignedRelayOnly(relay_url)
            } else {
                EndpointPolicy::TrustedRelayOnly(relay_url)
            };
            Ok((policy, true))
        }
    }
}

async fn bind_tunnel_listener(listen: Option<SocketAddr>) -> Result<TcpListener> {
    match listen {
        Some(address) => TcpListener::bind(address)
            .await
            .with_context(|| format!("listen on {address}")),
        None => bind_tunnel_listener_from(8080).await,
    }
}

pub(super) async fn bind_tunnel_listener_from(start_port: u16) -> Result<TcpListener> {
    for port in start_port..=u16::MAX {
        let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        if let Ok(listener) = TcpListener::bind(address).await {
            return Ok(listener);
        }
    }
    bail!("no available tunnel listen port from 127.0.0.1:{start_port} to 127.0.0.1:65535")
}

async fn connect_tunnel_stream(
    endpoint: Endpoint,
    mut endpoint_addr: iroh::EndpointAddr,
    relay_only: bool,
    access_key: [u8; 32],
    local: TcpStream,
) -> Result<()> {
    if relay_only {
        endpoint_addr = relay_only_addr(&endpoint_addr)
            .context("relay-only tunnel ticket has no relay address")?;
    }
    let trace = RecvTrace::new(false);
    let conn = connect_to_peer(&endpoint, endpoint_addr, relay_only, TUNNEL_ALPN, &trace).await?;
    let (mut send, mut recv) = conn.open_bi().await.context("open tunnel stream")?;
    let mut auth = [0u8; TUNNEL_AUTH_LEN];
    auth[..TUNNEL_AUTH_PREFIX.len()].copy_from_slice(TUNNEL_AUTH_PREFIX);
    auth[TUNNEL_AUTH_PREFIX.len()..].copy_from_slice(&access_key);
    send.write_all(&auth)
        .await
        .context("authenticate tunnel stream")?;
    let mut status = [0u8; 1];
    recv.read_exact(&mut status)
        .await
        .context("read tunnel status")?;
    match status[0] {
        TUNNEL_STATUS_OK => forward_tunnel_tcp(local, send, recv).await,
        TUNNEL_STATUS_BUSY => bail!("tunnel has reached its 64 connection limit"),
        _ => bail!("tunnel authentication or target connection was rejected"),
    }
}

async fn forward_tunnel_tcp(
    stream: TcpStream,
    mut send: iroh::endpoint::SendStream,
    mut recv: iroh::endpoint::RecvStream,
) -> Result<()> {
    let (mut stream_read, mut stream_write) = stream.into_split();
    let to_tunnel = async {
        io::copy(&mut stream_read, &mut send)
            .await
            .context("copy local TCP data to tunnel")?;
        send.finish().context("finish tunnel send stream")?;
        Ok::<(), anyhow::Error>(())
    };
    let from_tunnel = async {
        io::copy(&mut recv, &mut stream_write)
            .await
            .context("copy tunnel data to local TCP")?;
        stream_write
            .shutdown()
            .await
            .context("shutdown local TCP write")?;
        Ok::<(), anyhow::Error>(())
    };
    tokio::try_join!(to_tunnel, from_tunnel)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn tunnel_stream_forwards_tcp_data() {
        let target_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let target = target_listener.local_addr().unwrap().to_string();
        let echo = tokio::spawn(async move {
            let (stream, _) = target_listener.accept().await.unwrap();
            let (mut reader, mut writer) = stream.into_split();
            io::copy(&mut reader, &mut writer).await.unwrap();
        });

        let server = bind_endpoint(
            EndpointPolicy::standard(RelayMode::Disabled),
            TUNNEL_ALPN,
            None,
        )
        .await
        .unwrap();
        let server_addr = server.addr();
        let access_key = [7; 32];
        let served = tokio::spawn(async move {
            let incoming = server.accept().await.unwrap();
            let connection = incoming.accept().unwrap().await.unwrap();
            let (send, recv) = connection.accept_bi().await.unwrap();
            serve_tunnel_stream(send, recv, target, access_key, Arc::new(Semaphore::new(1)))
                .await
                .unwrap();
            connection.closed().await;
            server.close().await;
        });

        let client = bind_endpoint(
            EndpointPolicy::standard(RelayMode::Disabled),
            TUNNEL_ALPN,
            None,
        )
        .await
        .unwrap();
        let conn = client.connect(server_addr, TUNNEL_ALPN).await.unwrap();
        let (mut send, mut recv) = conn.open_bi().await.unwrap();
        let mut auth = [0u8; TUNNEL_AUTH_LEN];
        auth[..TUNNEL_AUTH_PREFIX.len()].copy_from_slice(TUNNEL_AUTH_PREFIX);
        auth[TUNNEL_AUTH_PREFIX.len()..].copy_from_slice(&access_key);
        send.write_all(&auth).await.unwrap();
        let mut status = [0u8; 1];
        recv.read_exact(&mut status).await.unwrap();
        assert_eq!(status[0], TUNNEL_STATUS_OK);
        send.write_all(b"tunnel payload").await.unwrap();
        send.finish().unwrap();
        assert_eq!(recv.read_to_end(64).await.unwrap(), b"tunnel payload");

        client.close().await;
        served.await.unwrap();
        echo.await.unwrap();
    }

    #[tokio::test]
    async fn tunnel_stream_rejects_invalid_access_key_before_target_connect() {
        let target_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let target = target_listener.local_addr().unwrap().to_string();
        let server = bind_endpoint(
            EndpointPolicy::standard(RelayMode::Disabled),
            TUNNEL_ALPN,
            None,
        )
        .await
        .unwrap();
        let server_addr = server.addr();
        let served = tokio::spawn(async move {
            let incoming = server.accept().await.unwrap();
            let connection = incoming.accept().unwrap().await.unwrap();
            let (send, recv) = connection.accept_bi().await.unwrap();
            serve_tunnel_stream(send, recv, target, [3; 32], Arc::new(Semaphore::new(1)))
                .await
                .unwrap();
            connection.closed().await;
            server.close().await;
        });

        let client = bind_endpoint(
            EndpointPolicy::standard(RelayMode::Disabled),
            TUNNEL_ALPN,
            None,
        )
        .await
        .unwrap();
        let conn = client.connect(server_addr, TUNNEL_ALPN).await.unwrap();
        let (mut send, mut recv) = conn.open_bi().await.unwrap();
        let mut auth = [0u8; TUNNEL_AUTH_LEN];
        auth[..TUNNEL_AUTH_PREFIX.len()].copy_from_slice(TUNNEL_AUTH_PREFIX);
        auth[TUNNEL_AUTH_PREFIX.len()..].copy_from_slice(&[4; 32]);
        send.write_all(&auth).await.unwrap();
        let mut status = [0u8; 1];
        recv.read_exact(&mut status).await.unwrap();
        assert_eq!(status[0], TUNNEL_STATUS_REJECTED);
        assert!(
            tokio::time::timeout(Duration::from_millis(100), target_listener.accept())
                .await
                .is_err()
        );

        client.close().await;
        served.await.unwrap();
    }

    #[tokio::test]
    async fn tunnel_default_listener_skips_an_occupied_port() {
        let occupied = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let port = occupied.local_addr().unwrap().port();
        if port < u16::MAX {
            let listener = bind_tunnel_listener_from(port).await.unwrap();
            assert!(listener.local_addr().unwrap().port() > port);
        }
    }
}
