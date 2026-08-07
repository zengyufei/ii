use crate::command::Socks5Args;
use anyhow::{Context, Result};
use std::{
    io,
    net::{IpAddr, Ipv4Addr, SocketAddr},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream, UdpSocket, lookup_host},
    time::{self, Duration, Instant},
};

const VERSION: u8 = 5;
const METHOD_NO_AUTH: u8 = 0;
const METHOD_USER_PASSWORD: u8 = 2;
const METHOD_NONE: u8 = 0xff;
const COMMAND_CONNECT: u8 = 1;
const COMMAND_BIND: u8 = 2;
const COMMAND_UDP_ASSOCIATE: u8 = 3;
const REPLY_SUCCEEDED: u8 = 0;
const REPLY_GENERAL_FAILURE: u8 = 1;
const REPLY_CONNECTION_NOT_ALLOWED: u8 = 2;
const REPLY_NETWORK_UNREACHABLE: u8 = 3;
const REPLY_HOST_UNREACHABLE: u8 = 4;
const REPLY_CONNECTION_REFUSED: u8 = 5;
const REPLY_TTL_EXPIRED: u8 = 6;
const REPLY_COMMAND_NOT_SUPPORTED: u8 = 7;
const REPLY_ADDRESS_TYPE_NOT_SUPPORTED: u8 = 8;
const UDP_IDLE_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const TCP_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const BIND_ACCEPT_TIMEOUT: Duration = Duration::from_secs(5 * 60);

#[derive(Clone)]
struct Credentials {
    username: Vec<u8>,
    password: Vec<u8>,
}

enum TargetAddr {
    Socket(SocketAddr),
    Domain(String, u16),
}

pub(super) async fn run(args: Socks5Args) -> Result<()> {
    let bind = args.bind.unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
    let listener = TcpListener::bind(SocketAddr::new(bind, args.port.unwrap_or(0)))
        .await
        .context("bind SOCKS5 listener")?;
    let address = listener
        .local_addr()
        .context("read SOCKS5 listener address")?;
    println!("ii socks5: socks5://{address}");
    println!("press Ctrl+C to stop proxy");
    let credentials = args
        .username
        .zip(args.password)
        .map(|(username, password)| Credentials {
            username: username.into_bytes(),
            password: password.into_bytes(),
        });
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            accepted = listener.accept() => {
                let (stream, _) = accepted.context("accept SOCKS5 connection")?;
                let credentials = credentials.clone();
                tokio::spawn(async move {
                    if let Err(err) = serve_connection(stream, credentials).await {
                        eprintln!("ii socks5: connection failed: {err:#}");
                    }
                });
            }
        }
    }
    Ok(())
}

async fn serve_connection(mut stream: TcpStream, credentials: Option<Credentials>) -> Result<()> {
    if !negotiate_auth(&mut stream, credentials.as_ref()).await? {
        return Ok(());
    }
    let mut request = [0u8; 3];
    stream
        .read_exact(&mut request)
        .await
        .context("read SOCKS5 request")?;
    if request[0] != VERSION || request[2] != 0 {
        write_reply(&mut stream, REPLY_GENERAL_FAILURE, None).await?;
        return Ok(());
    }
    let target = match read_target(&mut stream).await {
        Ok(target) => target,
        Err(_) => {
            write_reply(&mut stream, REPLY_ADDRESS_TYPE_NOT_SUPPORTED, None).await?;
            return Ok(());
        }
    };
    match request[1] {
        COMMAND_CONNECT => connect(&mut stream, target).await,
        COMMAND_BIND => bind(&mut stream, target).await,
        COMMAND_UDP_ASSOCIATE => udp_associate(&mut stream).await,
        _ => {
            write_reply(&mut stream, REPLY_COMMAND_NOT_SUPPORTED, None).await?;
            Ok(())
        }
    }
}

async fn negotiate_auth(stream: &mut TcpStream, credentials: Option<&Credentials>) -> Result<bool> {
    let mut greeting = [0u8; 2];
    stream
        .read_exact(&mut greeting)
        .await
        .context("read SOCKS5 greeting")?;
    if greeting[0] != VERSION {
        return Ok(false);
    }
    let mut methods = vec![0u8; greeting[1] as usize];
    stream
        .read_exact(&mut methods)
        .await
        .context("read SOCKS5 methods")?;
    let selected = if credentials.is_some() {
        methods
            .contains(&METHOD_USER_PASSWORD)
            .then_some(METHOD_USER_PASSWORD)
    } else {
        methods.contains(&METHOD_NO_AUTH).then_some(METHOD_NO_AUTH)
    };
    let Some(selected) = selected else {
        stream
            .write_all(&[VERSION, METHOD_NONE])
            .await
            .context("reject SOCKS5 method")?;
        return Ok(false);
    };
    stream
        .write_all(&[VERSION, selected])
        .await
        .context("select SOCKS5 method")?;
    let Some(credentials) = credentials else {
        return Ok(true);
    };
    let mut version_and_length = [0u8; 2];
    stream
        .read_exact(&mut version_and_length)
        .await
        .context("read SOCKS5 username")?;
    if version_and_length[0] != 1 {
        stream
            .write_all(&[1, 1])
            .await
            .context("reject SOCKS5 authentication")?;
        return Ok(false);
    }
    let mut username = vec![0u8; version_and_length[1] as usize];
    stream
        .read_exact(&mut username)
        .await
        .context("read SOCKS5 username")?;
    let mut password_length = [0u8; 1];
    stream
        .read_exact(&mut password_length)
        .await
        .context("read SOCKS5 password length")?;
    let mut password = vec![0u8; password_length[0] as usize];
    stream
        .read_exact(&mut password)
        .await
        .context("read SOCKS5 password")?;
    let accepted = username == credentials.username && password == credentials.password;
    stream
        .write_all(&[1, if accepted { 0 } else { 1 }])
        .await
        .context("write SOCKS5 authentication result")?;
    Ok(accepted)
}

async fn read_target(stream: &mut TcpStream) -> Result<TargetAddr> {
    let mut atyp = [0u8; 1];
    stream
        .read_exact(&mut atyp)
        .await
        .context("read SOCKS5 address type")?;
    match atyp[0] {
        1 => {
            let mut ip = [0u8; 4];
            stream
                .read_exact(&mut ip)
                .await
                .context("read SOCKS5 IPv4 address")?;
            let port = read_port(stream).await?;
            Ok(TargetAddr::Socket(SocketAddr::from((ip, port))))
        }
        3 => {
            let mut length = [0u8; 1];
            stream
                .read_exact(&mut length)
                .await
                .context("read SOCKS5 domain length")?;
            let mut domain = vec![0u8; length[0] as usize];
            stream
                .read_exact(&mut domain)
                .await
                .context("read SOCKS5 domain")?;
            let domain = String::from_utf8(domain).context("SOCKS5 domain is not UTF-8")?;
            let port = read_port(stream).await?;
            Ok(TargetAddr::Domain(domain, port))
        }
        4 => {
            let mut ip = [0u8; 16];
            stream
                .read_exact(&mut ip)
                .await
                .context("read SOCKS5 IPv6 address")?;
            let port = read_port(stream).await?;
            Ok(TargetAddr::Socket(SocketAddr::from((ip, port))))
        }
        _ => anyhow::bail!("unsupported SOCKS5 address type"),
    }
}

async fn read_port(stream: &mut TcpStream) -> Result<u16> {
    let mut port = [0u8; 2];
    stream
        .read_exact(&mut port)
        .await
        .context("read SOCKS5 port")?;
    Ok(u16::from_be_bytes(port))
}

async fn resolve_target(target: TargetAddr) -> io::Result<SocketAddr> {
    match target {
        TargetAddr::Socket(addr) => Ok(addr),
        TargetAddr::Domain(domain, port) => lookup_host((domain.as_str(), port))
            .await?
            .next()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "domain has no addresses")),
    }
}

async fn connect(stream: &mut TcpStream, target: TargetAddr) -> Result<()> {
    let target = match time::timeout(TCP_CONNECT_TIMEOUT, resolve_target(target)).await {
        Ok(Ok(target)) => target,
        Ok(Err(err)) => {
            write_reply(stream, reply_code(&err), None).await?;
            return Ok(());
        }
        Err(_) => {
            write_reply(stream, REPLY_TTL_EXPIRED, None).await?;
            return Ok(());
        }
    };
    let mut target_stream =
        match time::timeout(TCP_CONNECT_TIMEOUT, TcpStream::connect(target)).await {
            Ok(Ok(stream)) => stream,
            Ok(Err(err)) => {
                write_reply(stream, reply_code(&err), None).await?;
                return Ok(());
            }
            Err(_) => {
                write_reply(stream, REPLY_TTL_EXPIRED, None).await?;
                return Ok(());
            }
        };
    let bound = target_stream.local_addr().ok();
    write_reply(stream, REPLY_SUCCEEDED, bound).await?;
    tokio::io::copy_bidirectional(stream, &mut target_stream)
        .await
        .context("forward SOCKS5 CONNECT")?;
    Ok(())
}

async fn bind(stream: &mut TcpStream, target: TargetAddr) -> Result<()> {
    let expected_peer = match time::timeout(TCP_CONNECT_TIMEOUT, resolve_target(target)).await {
        Ok(Ok(target)) => target,
        Ok(Err(err)) => {
            write_reply(stream, reply_code(&err), None).await?;
            return Ok(());
        }
        Err(_) => {
            write_reply(stream, REPLY_TTL_EXPIRED, None).await?;
            return Ok(());
        }
    };
    let local = stream.local_addr().context("read SOCKS5 local address")?;
    let listener = TcpListener::bind(SocketAddr::new(local.ip(), 0))
        .await
        .context("bind SOCKS5 BIND listener")?;
    write_reply(stream, REPLY_SUCCEEDED, listener.local_addr().ok()).await?;
    let (mut incoming, remote) = loop {
        let accepted = match time::timeout(BIND_ACCEPT_TIMEOUT, listener.accept()).await {
            Ok(Ok(accepted)) => accepted,
            Ok(Err(err)) => return Err(err).context("accept SOCKS5 BIND connection"),
            Err(_) => {
                write_reply(stream, REPLY_TTL_EXPIRED, None).await?;
                return Ok(());
            }
        };
        if bind_peer_matches(expected_peer, accepted.1) {
            break accepted;
        }
    };
    write_reply(stream, REPLY_SUCCEEDED, Some(remote)).await?;
    tokio::io::copy_bidirectional(stream, &mut incoming)
        .await
        .context("forward SOCKS5 BIND")?;
    Ok(())
}

fn bind_peer_matches(expected: SocketAddr, peer: SocketAddr) -> bool {
    (expected.ip().is_unspecified() || expected.ip() == peer.ip())
        && (expected.port() == 0 || expected.port() == peer.port())
}

async fn udp_associate(stream: &mut TcpStream) -> Result<()> {
    udp_associate_with_timeout(stream, UDP_IDLE_TIMEOUT).await
}

async fn udp_associate_with_timeout(stream: &mut TcpStream, idle_timeout: Duration) -> Result<()> {
    let local = stream.local_addr().context("read SOCKS5 local address")?;
    let socket = UdpSocket::bind(SocketAddr::new(local.ip(), 0))
        .await
        .context("bind SOCKS5 UDP socket")?;
    write_reply(stream, REPLY_SUCCEEDED, socket.local_addr().ok()).await?;
    let control_peer = stream.peer_addr().context("read SOCKS5 peer address")?;
    let mut client_udp = None;
    let mut packet = vec![0u8; 64 * 1024 + 512];
    let mut control = [0u8; 1];
    let idle = time::sleep(idle_timeout);
    tokio::pin!(idle);
    loop {
        tokio::select! {
            _ = &mut idle => break,
            _ = stream.read(&mut control) => {
                break;
            }
            received = socket.recv_from(&mut packet) => {
                let (length, source) = match received {
                    Ok(value) => value,
                    Err(_) => break,
                };
                if client_udp.is_none() && source.ip() == control_peer.ip() {
                    client_udp = Some(source);
                }
                if Some(source) == client_udp {
                    if let Some((target, payload)) = parse_udp_request(&packet[..length]) {
                        if let Ok(target) = resolve_target(target).await {
                            let _ = socket.send_to(payload, target).await;
                            idle.as_mut().reset(Instant::now() + idle_timeout);
                        }
                    }
                } else if let Some(client) = client_udp {
                    let response = encode_udp_response(source, &packet[..length]);
                    let _ = socket.send_to(&response, client).await;
                    idle.as_mut().reset(Instant::now() + idle_timeout);
                }
            }
        }
    }
    Ok(())
}

fn parse_udp_request(packet: &[u8]) -> Option<(TargetAddr, &[u8])> {
    if packet.len() < 4 || packet[0] != 0 || packet[1] != 0 || packet[2] != 0 {
        return None;
    }
    let (target, offset) = parse_udp_target(packet, 3)?;
    Some((target, &packet[offset..]))
}

fn parse_udp_target(packet: &[u8], mut offset: usize) -> Option<(TargetAddr, usize)> {
    let atyp = *packet.get(offset)?;
    offset += 1;
    match atyp {
        1 => {
            let ip: [u8; 4] = packet.get(offset..offset + 4)?.try_into().ok()?;
            offset += 4;
            let port = u16::from_be_bytes(packet.get(offset..offset + 2)?.try_into().ok()?);
            Some((TargetAddr::Socket(SocketAddr::from((ip, port))), offset + 2))
        }
        3 => {
            let length = *packet.get(offset)? as usize;
            offset += 1;
            let domain = String::from_utf8(packet.get(offset..offset + length)?.to_vec()).ok()?;
            offset += length;
            let port = u16::from_be_bytes(packet.get(offset..offset + 2)?.try_into().ok()?);
            Some((TargetAddr::Domain(domain, port), offset + 2))
        }
        4 => {
            let ip: [u8; 16] = packet.get(offset..offset + 16)?.try_into().ok()?;
            offset += 16;
            let port = u16::from_be_bytes(packet.get(offset..offset + 2)?.try_into().ok()?);
            Some((TargetAddr::Socket(SocketAddr::from((ip, port))), offset + 2))
        }
        _ => None,
    }
}

fn encode_udp_response(source: SocketAddr, payload: &[u8]) -> Vec<u8> {
    let mut packet = vec![0, 0, 0];
    append_address(&mut packet, source);
    packet.extend_from_slice(payload);
    packet
}

async fn write_reply(stream: &mut TcpStream, code: u8, address: Option<SocketAddr>) -> Result<()> {
    let mut reply = vec![VERSION, code, 0];
    append_address(
        &mut reply,
        address.unwrap_or(SocketAddr::from(([0, 0, 0, 0], 0))),
    );
    stream.write_all(&reply).await.context("write SOCKS5 reply")
}

fn append_address(output: &mut Vec<u8>, address: SocketAddr) {
    match address {
        SocketAddr::V4(address) => {
            output.push(1);
            output.extend_from_slice(&address.ip().octets());
            output.extend_from_slice(&address.port().to_be_bytes());
        }
        SocketAddr::V6(address) => {
            output.push(4);
            output.extend_from_slice(&address.ip().octets());
            output.extend_from_slice(&address.port().to_be_bytes());
        }
    }
}

fn reply_code(error: &io::Error) -> u8 {
    match error.kind() {
        io::ErrorKind::ConnectionRefused => REPLY_CONNECTION_REFUSED,
        io::ErrorKind::NetworkUnreachable => REPLY_NETWORK_UNREACHABLE,
        io::ErrorKind::HostUnreachable | io::ErrorKind::NotFound => REPLY_HOST_UNREACHABLE,
        io::ErrorKind::TimedOut => REPLY_TTL_EXPIRED,
        io::ErrorKind::PermissionDenied => REPLY_CONNECTION_NOT_ALLOWED,
        _ => REPLY_GENERAL_FAILURE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn connect_forwards_tcp_without_authentication() {
        let target = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let target_address = target.local_addr().unwrap();
        let target_task = tokio::spawn(async move {
            let (mut stream, _) = target.accept().await.unwrap();
            let mut body = [0u8; 4];
            stream.read_exact(&mut body).await.unwrap();
            stream.write_all(&body).await.unwrap();
        });
        let proxy = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let proxy_address = proxy.local_addr().unwrap();
        let proxy_task = tokio::spawn(async move {
            let (stream, _) = proxy.accept().await.unwrap();
            serve_connection(stream, None).await.unwrap();
        });
        let mut client = TcpStream::connect(proxy_address).await.unwrap();
        client
            .write_all(&[VERSION, 1, METHOD_NO_AUTH])
            .await
            .unwrap();
        let mut selected = [0u8; 2];
        client.read_exact(&mut selected).await.unwrap();
        assert_eq!(selected, [VERSION, METHOD_NO_AUTH]);
        let mut request = vec![VERSION, COMMAND_CONNECT, 0, 1];
        if let SocketAddr::V4(address) = target_address {
            request.extend_from_slice(&address.ip().octets());
            request.extend_from_slice(&address.port().to_be_bytes());
        }
        client.write_all(&request).await.unwrap();
        let mut reply = [0u8; 10];
        client.read_exact(&mut reply).await.unwrap();
        assert_eq!(reply[0], VERSION);
        assert_eq!(reply[1], REPLY_SUCCEEDED);
        client.write_all(b"ping").await.unwrap();
        let mut echoed = [0u8; 4];
        client.read_exact(&mut echoed).await.unwrap();
        assert_eq!(&echoed, b"ping");
        drop(client);
        target_task.await.unwrap();
        proxy_task.await.unwrap();
    }

    #[tokio::test]
    async fn username_password_authentication_rejects_wrong_password() {
        let proxy = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let proxy_address = proxy.local_addr().unwrap();
        let proxy_task = tokio::spawn(async move {
            let (stream, _) = proxy.accept().await.unwrap();
            serve_connection(
                stream,
                Some(Credentials {
                    username: b"alice".to_vec(),
                    password: b"secret".to_vec(),
                }),
            )
            .await
            .unwrap();
        });
        let mut client = TcpStream::connect(proxy_address).await.unwrap();
        client
            .write_all(&[VERSION, 1, METHOD_USER_PASSWORD])
            .await
            .unwrap();
        let mut selected = [0u8; 2];
        client.read_exact(&mut selected).await.unwrap();
        assert_eq!(selected, [VERSION, METHOD_USER_PASSWORD]);
        client
            .write_all(&[
                1, 5, b'a', b'l', b'i', b'c', b'e', 4, b'v', b'a', b'l', b'e',
            ])
            .await
            .unwrap();
        let mut result = [0u8; 2];
        client.read_exact(&mut result).await.unwrap();
        assert_eq!(result, [1, 1]);
        proxy_task.await.unwrap();
    }

    #[tokio::test]
    async fn username_password_authentication_accepts_correct_password() {
        let proxy = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let proxy_address = proxy.local_addr().unwrap();
        let proxy_task = tokio::spawn(async move {
            let (mut stream, _) = proxy.accept().await.unwrap();
            assert!(
                negotiate_auth(
                    &mut stream,
                    Some(&Credentials {
                        username: b"alice".to_vec(),
                        password: b"secret".to_vec(),
                    }),
                )
                .await
                .unwrap()
            );
        });
        let mut client = TcpStream::connect(proxy_address).await.unwrap();
        client
            .write_all(&[VERSION, 1, METHOD_USER_PASSWORD])
            .await
            .unwrap();
        let mut selected = [0u8; 2];
        client.read_exact(&mut selected).await.unwrap();
        assert_eq!(selected, [VERSION, METHOD_USER_PASSWORD]);
        client
            .write_all(&[
                1, 5, b'a', b'l', b'i', b'c', b'e', 6, b's', b'e', b'c', b'r', b'e', b't',
            ])
            .await
            .unwrap();
        let mut result = [0u8; 2];
        client.read_exact(&mut result).await.unwrap();
        assert_eq!(result, [1, 0]);
        proxy_task.await.unwrap();
    }

    #[tokio::test]
    async fn unsupported_command_returns_standard_reply() {
        let proxy = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let proxy_address = proxy.local_addr().unwrap();
        let proxy_task = tokio::spawn(async move {
            let (stream, _) = proxy.accept().await.unwrap();
            serve_connection(stream, None).await.unwrap();
        });
        let mut client = TcpStream::connect(proxy_address).await.unwrap();
        negotiate_no_auth(&mut client).await;
        client
            .write_all(&[VERSION, 9, 0, 1, 0, 0, 0, 0, 0, 0])
            .await
            .unwrap();
        let (code, _) = read_reply(&mut client).await;
        assert_eq!(code, REPLY_COMMAND_NOT_SUPPORTED);
        proxy_task.await.unwrap();
    }

    #[tokio::test]
    async fn udp_associate_relays_datagrams() {
        let target = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let target_address = target.local_addr().unwrap();
        let target_task = tokio::spawn(async move {
            let mut packet = [0u8; 128];
            let (length, peer) = target.recv_from(&mut packet).await.unwrap();
            target.send_to(&packet[..length], peer).await.unwrap();
        });
        let proxy = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let proxy_address = proxy.local_addr().unwrap();
        let proxy_task = tokio::spawn(async move {
            let (stream, _) = proxy.accept().await.unwrap();
            serve_connection(stream, None).await.unwrap();
        });
        let mut client = TcpStream::connect(proxy_address).await.unwrap();
        negotiate_no_auth(&mut client).await;
        client
            .write_all(&[VERSION, COMMAND_UDP_ASSOCIATE, 0, 1, 0, 0, 0, 0, 0, 0])
            .await
            .unwrap();
        let (_, udp_proxy) = read_reply(&mut client).await;
        let udp_client = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let mut request = vec![0, 0, 0];
        append_address(&mut request, target_address);
        request.extend_from_slice(b"udp");
        udp_client.send_to(&request, udp_proxy).await.unwrap();
        let mut response = [0u8; 128];
        let (length, _) = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            udp_client.recv_from(&mut response),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(&response[..3], &[0, 0, 0]);
        assert_eq!(&response[length - 3..length], b"udp");
        drop(client);
        target_task.await.unwrap();
        proxy_task.await.unwrap();
    }

    #[tokio::test]
    async fn udp_associate_closes_after_idle_timeout() {
        let proxy = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let proxy_address = proxy.local_addr().unwrap();
        let proxy_task = tokio::spawn(async move {
            let (mut stream, _) = proxy.accept().await.unwrap();
            negotiate_auth(&mut stream, None).await.unwrap();
            let mut request = [0u8; 3];
            stream.read_exact(&mut request).await.unwrap();
            let _ = read_target(&mut stream).await.unwrap();
            assert_eq!(request, [VERSION, COMMAND_UDP_ASSOCIATE, 0]);
            udp_associate_with_timeout(&mut stream, Duration::from_millis(20))
                .await
                .unwrap();
        });
        let mut client = TcpStream::connect(proxy_address).await.unwrap();
        negotiate_no_auth(&mut client).await;
        client
            .write_all(&[VERSION, COMMAND_UDP_ASSOCIATE, 0, 1, 0, 0, 0, 0, 0, 0])
            .await
            .unwrap();
        let (_, _) = read_reply(&mut client).await;
        let mut eof = [0u8; 1];
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), client.read(&mut eof))
                .await
                .unwrap()
                .unwrap(),
            0
        );
        proxy_task.await.unwrap();
    }

    #[tokio::test]
    async fn bind_returns_two_replies_and_forwards_tcp() {
        let proxy = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let proxy_address = proxy.local_addr().unwrap();
        let proxy_task = tokio::spawn(async move {
            let (stream, _) = proxy.accept().await.unwrap();
            serve_connection(stream, None).await.unwrap();
        });
        let mut client = TcpStream::connect(proxy_address).await.unwrap();
        negotiate_no_auth(&mut client).await;
        client
            .write_all(&[VERSION, COMMAND_BIND, 0, 1, 0, 0, 0, 0, 0, 0])
            .await
            .unwrap();
        let (_, bind_address) = read_reply(&mut client).await;
        let mut incoming = TcpStream::connect(bind_address).await.unwrap();
        let (code, _) = read_reply(&mut client).await;
        assert_eq!(code, REPLY_SUCCEEDED);
        client.write_all(b"bind").await.unwrap();
        let mut received = [0u8; 4];
        incoming.read_exact(&mut received).await.unwrap();
        assert_eq!(&received, b"bind");
        incoming.write_all(b"done").await.unwrap();
        client.read_exact(&mut received).await.unwrap();
        assert_eq!(&received, b"done");
        drop(incoming);
        drop(client);
        proxy_task.await.unwrap();
    }

    async fn negotiate_no_auth(stream: &mut TcpStream) {
        stream
            .write_all(&[VERSION, 1, METHOD_NO_AUTH])
            .await
            .unwrap();
        let mut selected = [0u8; 2];
        stream.read_exact(&mut selected).await.unwrap();
        assert_eq!(selected, [VERSION, METHOD_NO_AUTH]);
    }

    async fn read_reply(stream: &mut TcpStream) -> (u8, SocketAddr) {
        let mut header = [0u8; 4];
        stream.read_exact(&mut header).await.unwrap();
        assert_eq!(header[0], VERSION);
        let address = match header[3] {
            1 => {
                let mut value = [0u8; 6];
                stream.read_exact(&mut value).await.unwrap();
                SocketAddr::from((
                    [value[0], value[1], value[2], value[3]],
                    u16::from_be_bytes([value[4], value[5]]),
                ))
            }
            4 => {
                let mut value = [0u8; 18];
                stream.read_exact(&mut value).await.unwrap();
                SocketAddr::from((
                    [
                        value[0], value[1], value[2], value[3], value[4], value[5], value[6],
                        value[7], value[8], value[9], value[10], value[11], value[12], value[13],
                        value[14], value[15],
                    ],
                    u16::from_be_bytes([value[16], value[17]]),
                ))
            }
            other => panic!("unexpected reply address type {other}"),
        };
        (header[1], address)
    }
}
