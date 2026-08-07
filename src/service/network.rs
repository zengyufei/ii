use crate::{
    command::{ForwardArgs, HealthArgs, PingArgs, PortArgs, ProxyArgs, WakeArgs},
    transport::progress::fmt_bytes,
    web::http::{WebRequest, bind_lan_web_listener, read_web_request},
};
use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD};
use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{self, AsyncReadExt, AsyncWriteExt},
    net::{TcpStream, UdpSocket, lookup_host},
    sync::{Mutex, mpsc},
    time::{self, Instant},
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const UDP_IDLE_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const SPEED_CHUNK: usize = 1024 * 1024;

#[derive(Clone)]
struct ProxyCredentials {
    value: Vec<u8>,
}

pub(super) async fn proxy(args: ProxyArgs) -> Result<()> {
    let listener = bind_lan_web_listener(args.port, args.bind, "HTTP proxy").await?;
    let address = listener.local_addr().context("read HTTP proxy listener")?;
    println!("ii proxy: http://{address}");
    println!("press Ctrl+C to stop proxy");
    let credentials = args
        .username
        .zip(args.password)
        .map(|(username, password)| ProxyCredentials {
            value: format!("{username}:{password}").into_bytes(),
        });
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            accepted = listener.accept() => {
                let (stream, _) = accepted.context("accept HTTP proxy connection")?;
                let credentials = credentials.clone();
                tokio::spawn(async move {
                    if let Err(err) = serve_proxy_connection(stream, credentials).await {
                        eprintln!("ii proxy: connection failed: {err:#}");
                    }
                });
            }
        }
    }
    Ok(())
}

async fn serve_proxy_connection(
    mut client: TcpStream,
    credentials: Option<ProxyCredentials>,
) -> Result<()> {
    let request = match read_web_request(&mut client).await {
        Ok(request) => request,
        Err(err) => {
            return write_proxy_error(&mut client, "400 Bad Request", &err.to_string()).await;
        }
    };
    if !proxy_authorized(&request, credentials.as_ref()) {
        return write_proxy_auth_required(&mut client).await;
    }
    if request.method.eq_ignore_ascii_case("CONNECT") {
        let target = match resolve_target(&request.target).await {
            Ok(target) => target,
            Err(_) => {
                return write_proxy_error(&mut client, "400 Bad Request", "invalid CONNECT target")
                    .await;
            }
        };
        let mut upstream = match time::timeout(CONNECT_TIMEOUT, TcpStream::connect(target)).await {
            Ok(Ok(stream)) => stream,
            Ok(Err(err)) => {
                return write_proxy_error(&mut client, "502 Bad Gateway", &err.to_string()).await;
            }
            Err(_) => {
                return write_proxy_error(&mut client, "504 Gateway Timeout", "connect timed out")
                    .await;
            }
        };
        client
            .write_all(b"HTTP/1.1 200 Connection Established\r\nConnection: close\r\n\r\n")
            .await
            .context("write CONNECT response")?;
        io::copy_bidirectional(&mut client, &mut upstream)
            .await
            .context("proxy CONNECT traffic")?;
        return Ok(());
    }

    let url = match url::Url::parse(&request.target) {
        Ok(url) if url.scheme() == "http" && url.host_str().is_some() => url,
        _ => {
            return write_proxy_error(
                &mut client,
                "400 Bad Request",
                "request target must be an http URL",
            )
            .await;
        }
    };
    let target = match resolve_url_target(&url).await {
        Ok(target) => target,
        Err(err) => {
            return write_proxy_error(&mut client, "502 Bad Gateway", &err.to_string()).await;
        }
    };
    let mut upstream = match time::timeout(CONNECT_TIMEOUT, TcpStream::connect(target)).await {
        Ok(Ok(stream)) => stream,
        Ok(Err(err)) => {
            return write_proxy_error(&mut client, "502 Bad Gateway", &err.to_string()).await;
        }
        Err(_) => {
            return write_proxy_error(&mut client, "504 Gateway Timeout", "connect timed out")
                .await;
        }
    };
    forward_proxy_request(&mut client, &mut upstream, &request, &url).await?;
    upstream.shutdown().await.context("finish proxy request")?;
    io::copy(&mut upstream, &mut client)
        .await
        .context("forward proxy response")?;
    client.shutdown().await.context("finish proxy response")?;
    Ok(())
}

fn proxy_authorized(request: &WebRequest, credentials: Option<&ProxyCredentials>) -> bool {
    let Some(credentials) = credentials else {
        return true;
    };
    let Some(value) = request.header("proxy-authorization") else {
        return false;
    };
    let Some((scheme, encoded)) = value.split_once(' ') else {
        return false;
    };
    if !scheme.eq_ignore_ascii_case("basic") || encoded.contains(char::is_whitespace) {
        return false;
    }
    let Ok(value) = STANDARD.decode(encoded) else {
        return false;
    };
    constant_time_eq(&value, &credentials.value)
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .fold(0u8, |difference, (left, right)| difference | (left ^ right))
            == 0
}

async fn write_proxy_auth_required(stream: &mut TcpStream) -> Result<()> {
    stream
        .write_all(b"HTTP/1.1 407 Proxy Authentication Required\r\nProxy-Authenticate: Basic realm=\"ii\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
        .await
        .context("write proxy authentication response")?;
    stream
        .shutdown()
        .await
        .context("finish proxy authentication response")?;
    Ok(())
}

async fn write_proxy_error(stream: &mut TcpStream, status: &str, message: &str) -> Result<()> {
    let body = message.as_bytes();
    stream
        .write_all(
            format!(
                "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .as_bytes(),
        )
        .await
        .context("write proxy error headers")?;
    stream
        .write_all(body)
        .await
        .context("write proxy error body")?;
    stream.shutdown().await.context("finish proxy error")?;
    Ok(())
}

async fn forward_proxy_request(
    client: &mut TcpStream,
    upstream: &mut TcpStream,
    request: &WebRequest,
    url: &url::Url,
) -> Result<()> {
    let mut path = url.path().to_string();
    if path.is_empty() {
        path.push('/');
    }
    if let Some(query) = url.query() {
        path.push('?');
        path.push_str(query);
    }
    let host = url.host_str().context("proxy target host is missing")?;
    let authority = http_authority(host, url.port());
    let mut output = format!(
        "{} {path} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n",
        request.method
    );
    let mut content_length = None;
    let mut chunked = false;
    for (name, value) in &request.headers {
        match name.as_str() {
            "host" | "connection" | "proxy-connection" | "proxy-authorization" => {}
            "content-length" => {
                content_length = Some(value.parse::<u64>().context("invalid Content-Length")?);
                output.push_str(&format!("Content-Length: {value}\r\n"));
            }
            "transfer-encoding" if value.eq_ignore_ascii_case("chunked") => {
                chunked = true;
                output.push_str("Transfer-Encoding: chunked\r\n");
            }
            "transfer-encoding" => bail!("unsupported request Transfer-Encoding"),
            _ => output.push_str(&format!("{name}: {value}\r\n")),
        }
    }
    if chunked && content_length.is_some() {
        bail!("request cannot use both Content-Length and Transfer-Encoding");
    }
    output.push_str("\r\n");
    upstream
        .write_all(output.as_bytes())
        .await
        .context("write proxy request headers")?;
    if chunked {
        relay_chunked(client, upstream, request.body.clone()).await
    } else if let Some(content_length) = content_length {
        relay_exact(client, upstream, request.body.clone(), content_length).await
    } else if !request.body.is_empty() {
        bail!("request body has no framing header");
    } else {
        Ok(())
    }
}

async fn relay_exact(
    input: &mut TcpStream,
    output: &mut TcpStream,
    initial: Vec<u8>,
    length: u64,
) -> Result<()> {
    if u64::try_from(initial.len()).unwrap_or(u64::MAX) > length {
        bail!("request body exceeds Content-Length");
    }
    output
        .write_all(&initial)
        .await
        .context("forward request body")?;
    let remaining = length - u64::try_from(initial.len()).unwrap_or(0);
    let copied = io::copy(&mut input.take(remaining), output)
        .await
        .context("forward request body")?;
    if copied != remaining {
        bail!("request body ended early");
    }
    Ok(())
}

async fn relay_chunked(
    input: &mut TcpStream,
    output: &mut TcpStream,
    mut pending: Vec<u8>,
) -> Result<()> {
    loop {
        let line = read_line(input, &mut pending).await?;
        let length_text = std::str::from_utf8(&line)
            .context("chunk header is not UTF-8")?
            .split(';')
            .next()
            .context("chunk length is missing")?;
        let length = u64::from_str_radix(length_text, 16).context("chunk length is invalid")?;
        output
            .write_all(&line)
            .await
            .context("forward chunk header")?;
        output
            .write_all(b"\r\n")
            .await
            .context("finish chunk header")?;
        if length == 0 {
            loop {
                let trailer = read_line(input, &mut pending).await?;
                output
                    .write_all(&trailer)
                    .await
                    .context("forward chunk trailer")?;
                output
                    .write_all(b"\r\n")
                    .await
                    .context("finish chunk trailer")?;
                if trailer.is_empty() {
                    return Ok(());
                }
            }
        }
        relay_buffered(input, output, &mut pending, length).await?;
        let ending = read_exact_buffered(input, &mut pending, 2).await?;
        if ending != b"\r\n" {
            bail!("chunk body is invalid");
        }
        output
            .write_all(b"\r\n")
            .await
            .context("finish chunk body")?;
    }
}

async fn relay_buffered(
    input: &mut TcpStream,
    output: &mut TcpStream,
    pending: &mut Vec<u8>,
    length: u64,
) -> Result<()> {
    let mut remaining = usize::try_from(length).context("chunk is too large")?;
    while remaining > 0 {
        if pending.is_empty() {
            fill_pending(input, pending).await?;
        }
        let take = remaining.min(pending.len());
        output
            .write_all(&pending[..take])
            .await
            .context("forward chunk body")?;
        pending.drain(..take);
        remaining -= take;
    }
    Ok(())
}

pub(super) async fn tcp(args: ForwardArgs) -> Result<()> {
    let listener = bind_lan_web_listener(args.port, args.bind, "TCP forwarder").await?;
    let address = listener
        .local_addr()
        .context("read TCP forwarder listener")?;
    println!("ii tcp: {address} -> {}", args.target);
    println!("press Ctrl+C to stop forwarding");
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            accepted = listener.accept() => {
                let (mut client, _) = accepted.context("accept TCP forwarding connection")?;
                let target = args.target.clone();
                tokio::spawn(async move {
                    match TcpStream::connect(&target).await {
                        Ok(mut upstream) => {
                            if let Err(err) = io::copy_bidirectional(&mut client, &mut upstream).await {
                                eprintln!("ii tcp: forward failed: {err}");
                            }
                        }
                        Err(err) => eprintln!("ii tcp: connect {target} failed: {err}"),
                    }
                });
            }
        }
    }
    Ok(())
}

struct UdpSession {
    sender: mpsc::Sender<Vec<u8>>,
    activity: Instant,
}

pub(super) async fn udp(args: ForwardArgs) -> Result<()> {
    let socket = Arc::new(bind_udp_listener(args.port, args.bind).await?);
    let address = socket.local_addr().context("read UDP forwarder listener")?;
    println!("ii udp: {address} -> {}", args.target);
    println!("press Ctrl+C to stop forwarding");
    let sessions = Arc::new(Mutex::new(HashMap::<SocketAddr, UdpSession>::new()));
    let mut cleanup = time::interval(Duration::from_secs(60));
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            _ = cleanup.tick() => {
                sessions.lock().await.retain(|_, session| session.activity.elapsed() < UDP_IDLE_TIMEOUT);
            }
            received = socket.recv_from(&mut buf) => {
                let (length, peer) = received.context("read UDP forwarding packet")?;
                let packet = buf[..length].to_vec();
                let sender = {
                    let mut guard = sessions.lock().await;
                    if let Some(session) = guard.get_mut(&peer) {
                        session.activity = Instant::now();
                        session.sender.clone()
                    } else {
                        let (sender, receiver) = mpsc::channel(64);
                        guard.insert(peer, UdpSession { sender: sender.clone(), activity: Instant::now() });
                        let socket = Arc::clone(&socket);
                        let target = args.target.clone();
                        tokio::spawn(async move {
                            if let Err(err) = run_udp_session(socket, peer, target, receiver).await {
                                eprintln!("ii udp: session {peer} failed: {err:#}");
                            }
                        });
                        sender
                    }
                };
                if sender.send(packet).await.is_err() {
                    sessions.lock().await.remove(&peer);
                }
            }
        }
    }
    Ok(())
}

async fn run_udp_session(
    listener: Arc<UdpSocket>,
    peer: SocketAddr,
    target: String,
    mut receiver: mpsc::Receiver<Vec<u8>>,
) -> Result<()> {
    let target = resolve_target(&target).await?;
    let upstream = UdpSocket::bind(SocketAddr::new(unspecified_for(target.ip()), 0))
        .await
        .context("bind UDP upstream")?;
    upstream
        .connect(target)
        .await
        .context("connect UDP upstream")?;
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        tokio::select! {
            _ = time::sleep(UDP_IDLE_TIMEOUT) => break,
            packet = receiver.recv() => match packet {
                Some(packet) => {
                    upstream.send(&packet).await.context("forward UDP packet")?;
                }
                None => break,
            },
            received = upstream.recv(&mut buf) => {
                let length = received.context("read UDP upstream packet")?;
                listener.send_to(&buf[..length], peer).await.context("return UDP packet")?;
            }
        }
    }
    Ok(())
}

fn unspecified_for(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V4(_) => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        IpAddr::V6(_) => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
    }
}

pub(super) async fn ping(args: PingArgs) -> Result<()> {
    let mut values = Vec::new();
    let mut failed = false;
    for index in 0..args.count {
        let started = Instant::now();
        match time::timeout(args.timeout, TcpStream::connect(&args.target)).await {
            Ok(Ok(stream)) => {
                let elapsed = started.elapsed();
                drop(stream);
                println!("{}: {} ms", index + 1, elapsed.as_secs_f64() * 1000.0);
                values.push(elapsed);
            }
            Ok(Err(err)) => {
                println!("{}: failed ({err})", index + 1);
                failed = true;
            }
            Err(_) => {
                println!("{}: timeout", index + 1);
                failed = true;
            }
        }
        if index + 1 < args.count {
            time::sleep(args.interval).await;
        }
    }
    if !values.is_empty() {
        let min = values.iter().min().expect("not empty").as_secs_f64() * 1000.0;
        let max = values.iter().max().expect("not empty").as_secs_f64() * 1000.0;
        let avg =
            values.iter().map(Duration::as_secs_f64).sum::<f64>() * 1000.0 / values.len() as f64;
        println!("min/avg/max: {min:.2}/{avg:.2}/{max:.2} ms");
    }
    if failed {
        bail!("one or more TCP probes failed");
    }
    Ok(())
}

pub(super) async fn speed(url: String, duration: Duration) -> Result<()> {
    let started = Instant::now();
    let download = speed_download(&url, duration).await?;
    let upload = speed_upload(&url, duration).await?;
    println!(
        "download: {} ({}/s)",
        fmt_bytes(download.0),
        fmt_bytes(rate(download.0, download.1))
    );
    println!(
        "upload: {} ({}/s)",
        fmt_bytes(upload.0),
        fmt_bytes(rate(upload.0, upload.1))
    );
    println!("total: {:.2}s", started.elapsed().as_secs_f64());
    Ok(())
}

fn rate(bytes: u64, elapsed: Duration) -> u64 {
    (bytes as f64 / elapsed.as_secs_f64().max(f64::EPSILON)) as u64
}

async fn speed_download(base: &str, duration: Duration) -> Result<(u64, Duration)> {
    let url = speed_endpoint(base, "download", duration)?;
    let mut stream = connect_http_url(&url).await?;
    write_http_request(&mut stream, "GET", &url, "").await?;
    let started = Instant::now();
    let (status, headers, pending) = read_http_headers(&mut stream).await?;
    if status != 200 || !header_equals(&headers, "transfer-encoding", "chunked") {
        bail!("speed download endpoint rejected the request");
    }
    let bytes = read_chunked_count(&mut stream, pending).await?;
    Ok((bytes, started.elapsed()))
}

async fn speed_upload(base: &str, duration: Duration) -> Result<(u64, Duration)> {
    let url = speed_endpoint(base, "upload", duration)?;
    let mut stream = connect_http_url(&url).await?;
    write_http_request(&mut stream, "POST", &url, "Transfer-Encoding: chunked\r\n").await?;
    let started = Instant::now();
    let data = vec![0u8; SPEED_CHUNK];
    let deadline = Instant::now() + duration;
    let mut bytes = 0u64;
    while Instant::now() < deadline {
        stream
            .write_all(format!("{:X}\r\n", data.len()).as_bytes())
            .await
            .context("write speed upload chunk header")?;
        stream
            .write_all(&data)
            .await
            .context("write speed upload chunk")?;
        stream
            .write_all(b"\r\n")
            .await
            .context("finish speed upload chunk")?;
        bytes = bytes
            .checked_add(data.len() as u64)
            .context("speed upload is too large")?;
    }
    stream
        .write_all(b"0\r\n\r\n")
        .await
        .context("finish speed upload")?;
    let (status, _, _) = read_http_headers(&mut stream).await?;
    if status != 200 {
        bail!("speed upload endpoint rejected the request");
    }
    Ok((bytes, started.elapsed()))
}

fn speed_endpoint(base: &str, endpoint: &str, duration: Duration) -> Result<url::Url> {
    let mut url = url::Url::parse(base).context("parse speed URL")?;
    if url.scheme() != "http" || url.host_str().is_none() {
        bail!("speed URL must use http://");
    }
    if !url.path().ends_with('/') {
        let path = format!("{}/", url.path());
        url.set_path(&path);
    }
    url = url.join(endpoint).context("build speed endpoint URL")?;
    url.set_query(Some(&format!("duration_ms={}", duration.as_millis())));
    Ok(url)
}

pub(super) async fn wake(args: WakeArgs) -> Result<()> {
    let bind = SocketAddr::new(unspecified_for(args.broadcast), 0);
    let socket = UdpSocket::bind(bind)
        .await
        .context("bind Wake-on-LAN socket")?;
    if args.broadcast.is_ipv4() {
        socket
            .set_broadcast(true)
            .context("enable Wake-on-LAN broadcast")?;
    }
    let mut packet = Vec::with_capacity(102);
    packet.extend_from_slice(&[0xff; 6]);
    for _ in 0..16 {
        packet.extend_from_slice(&args.mac);
    }
    socket
        .send_to(&packet, SocketAddr::new(args.broadcast, args.port))
        .await
        .context("send Wake-on-LAN packet")?;
    println!("ii wake: sent to {}:{}", args.broadcast, args.port);
    Ok(())
}

pub(super) async fn port(args: PortArgs) -> Result<()> {
    let mut tasks = Vec::new();
    for port in &args.ports {
        let host = args.host.clone();
        let timeout = args.timeout;
        let port = *port;
        tasks.push(tokio::spawn(async move {
            (port, check_host_port(&host, port, timeout).await)
        }));
    }
    let mut failed = false;
    for task in tasks {
        let (port, result) = task.await.context("join port check")?;
        match result {
            Ok(()) => println!("{port}: open"),
            Err(CheckError::Timeout) => {
                println!("{port}: timeout");
                failed = true;
            }
            Err(CheckError::Closed) => {
                println!("{port}: closed");
                failed = true;
            }
        }
    }
    if failed {
        bail!("one or more ports are not reachable");
    }
    Ok(())
}

enum CheckError {
    Timeout,
    Closed,
}

async fn check_host_port(host: &str, port: u16, timeout: Duration) -> Result<(), CheckError> {
    let addresses = lookup_host((host, port))
        .await
        .map_err(|_| CheckError::Closed)?;
    let mut found = false;
    for address in addresses {
        found = true;
        match time::timeout(timeout, TcpStream::connect(address)).await {
            Ok(Ok(stream)) => {
                drop(stream);
                return Ok(());
            }
            Err(_) => return Err(CheckError::Timeout),
            Ok(Err(_)) => {}
        }
    }
    if found {
        Err(CheckError::Closed)
    } else {
        Err(CheckError::Closed)
    }
}

pub(super) async fn health(args: HealthArgs) -> Result<()> {
    let mut last = None;
    loop {
        let result = check_health(&args.target, args.timeout).await;
        let healthy = result.is_ok();
        if args.interval.is_none() || last != Some(healthy) {
            match &result {
                Ok(()) => println!("healthy: {}", args.target),
                Err(err) => println!("unhealthy: {} ({err:#})", args.target),
            }
        }
        if args.interval.is_none() {
            return result;
        }
        last = Some(healthy);
        tokio::select! {
            _ = tokio::signal::ctrl_c() => return Ok(()),
            _ = time::sleep(args.interval.expect("checked")) => {}
        }
    }
}

async fn check_health(target: &str, timeout: Duration) -> Result<()> {
    if target.starts_with("http://") || target.starts_with("https://") {
        let target = target.to_string();
        let response = time::timeout(
            timeout,
            tokio::task::spawn_blocking(move || attohttpc::get(&target).timeout(timeout).send()),
        )
        .await
        .context("health request timed out")?
        .context("join health request")?
        .context("health HTTP request failed")?;
        let status = response.status();
        if status.is_success() || status.is_redirection() {
            return Ok(());
        }
        bail!("HTTP status {status}");
    }
    time::timeout(timeout, TcpStream::connect(target))
        .await
        .context("TCP health check timed out")?
        .context("TCP health check failed")?;
    Ok(())
}

async fn resolve_target(value: &str) -> Result<SocketAddr> {
    let (host, port) = split_target(value)?;
    lookup_host((host.as_str(), port))
        .await
        .context("resolve target")?
        .next()
        .context("target has no addresses")
}

fn split_target(value: &str) -> Result<(String, u16)> {
    if let Some(value) = value.strip_prefix('[') {
        let (host, port) = value.split_once("]:").context("invalid IPv6 target")?;
        return Ok((
            host.to_string(),
            port.parse().context("invalid target port")?,
        ));
    }
    let (host, port) = value.rsplit_once(':').context("target must be host:port")?;
    Ok((
        host.to_string(),
        port.parse().context("invalid target port")?,
    ))
}

async fn resolve_url_target(url: &url::Url) -> Result<SocketAddr> {
    let host = url.host_str().context("URL host is missing")?;
    let port = url.port_or_known_default().context("URL port is missing")?;
    lookup_host((host, port))
        .await
        .context("resolve URL host")?
        .next()
        .context("URL host has no addresses")
}

async fn connect_http_url(url: &url::Url) -> Result<TcpStream> {
    let target = resolve_url_target(url).await?;
    TcpStream::connect(target)
        .await
        .context("connect speed server")
}

async fn write_http_request(
    stream: &mut TcpStream,
    method: &str,
    url: &url::Url,
    extra_headers: &str,
) -> Result<()> {
    let host = url.host_str().context("URL host is missing")?;
    let authority = http_authority(host, url.port());
    let mut path = url.path().to_string();
    if path.is_empty() {
        path.push('/');
    }
    if let Some(query) = url.query() {
        path.push('?');
        path.push_str(query);
    }
    stream
        .write_all(format!("{method} {path} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n{extra_headers}\r\n").as_bytes())
        .await
        .context("write speed request")
}

fn http_authority(host: &str, port: Option<u16>) -> String {
    let host = if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_string()
    };
    match port {
        Some(port) => format!("{host}:{port}"),
        None => host,
    }
}

async fn bind_udp_listener(port: Option<u16>, bind: Option<IpAddr>) -> Result<UdpSocket> {
    let address = SocketAddr::new(
        bind.unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED)),
        port.unwrap_or(0),
    );
    let socket = socket2::Socket::new(
        socket2::Domain::for_address(address),
        socket2::Type::DGRAM,
        Some(socket2::Protocol::UDP),
    )
    .context("create UDP forwarder socket")?;
    socket
        .set_reuse_address(true)
        .context("configure UDP forwarder socket")?;
    if address.is_ipv6() {
        socket
            .set_only_v6(true)
            .context("configure IPv6 UDP forwarder socket")?;
    }
    socket.bind(&address.into()).context("bind UDP forwarder")?;
    socket
        .set_nonblocking(true)
        .context("configure UDP forwarder")?;
    UdpSocket::from_std(socket.into()).context("start UDP forwarder")
}

async fn read_http_headers(
    stream: &mut TcpStream,
) -> Result<(u16, Vec<(String, String)>, Vec<u8>)> {
    let mut bytes = Vec::with_capacity(1024);
    let mut buf = [0u8; 4096];
    let end = loop {
        if let Some(position) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
        if bytes.len() > 16 * 1024 {
            bail!("HTTP response headers exceed 16 KiB");
        }
        let read = stream.read(&mut buf).await.context("read HTTP response")?;
        if read == 0 {
            bail!("HTTP response ended before headers");
        }
        bytes.extend_from_slice(&buf[..read]);
    };
    let head = std::str::from_utf8(&bytes[..end - 4]).context("HTTP response is not UTF-8")?;
    let mut lines = head.split("\r\n");
    let status = lines.next().context("HTTP status line is missing")?;
    let code = status
        .split_ascii_whitespace()
        .nth(1)
        .context("HTTP status code is missing")?
        .parse::<u16>()
        .context("HTTP status code is invalid")?;
    let mut headers = Vec::new();
    for line in lines {
        let (name, value) = line
            .split_once(':')
            .context("HTTP response header is invalid")?;
        headers.push((name.to_ascii_lowercase(), value.trim().to_string()));
    }
    Ok((code, headers, bytes.split_off(end)))
}

fn header_equals(headers: &[(String, String)], expected_name: &str, expected: &str) -> bool {
    headers.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case(expected_name) && value.eq_ignore_ascii_case(expected)
    })
}

async fn read_chunked_count(stream: &mut TcpStream, mut pending: Vec<u8>) -> Result<u64> {
    let mut total = 0u64;
    loop {
        let line = read_line(stream, &mut pending).await?;
        let length = std::str::from_utf8(&line)
            .context("chunk length is not UTF-8")?
            .split(';')
            .next()
            .context("chunk length is missing")?;
        let length = u64::from_str_radix(length, 16).context("chunk length is invalid")?;
        if length == 0 {
            loop {
                if read_line(stream, &mut pending).await?.is_empty() {
                    return Ok(total);
                }
            }
        }
        discard(stream, &mut pending, length).await?;
        let ending = read_exact_buffered(stream, &mut pending, 2).await?;
        if ending != b"\r\n" {
            bail!("chunk body is invalid");
        }
        total = total
            .checked_add(length)
            .context("speed download is too large")?;
    }
}

async fn read_line(stream: &mut TcpStream, pending: &mut Vec<u8>) -> Result<Vec<u8>> {
    loop {
        if let Some(position) = pending.windows(2).position(|window| window == b"\r\n") {
            let line = pending.drain(..position).collect();
            pending.drain(..2);
            return Ok(line);
        }
        if pending.len() > 16 * 1024 {
            bail!("chunk header is too large");
        }
        fill_pending(stream, pending).await?;
    }
}

async fn discard(stream: &mut TcpStream, pending: &mut Vec<u8>, length: u64) -> Result<()> {
    let mut remaining = usize::try_from(length).context("chunk is too large")?;
    while remaining > 0 {
        if pending.is_empty() {
            fill_pending(stream, pending).await?;
        }
        let take = remaining.min(pending.len());
        pending.drain(..take);
        remaining -= take;
    }
    Ok(())
}

async fn read_exact_buffered(
    stream: &mut TcpStream,
    pending: &mut Vec<u8>,
    length: usize,
) -> Result<Vec<u8>> {
    while pending.len() < length {
        fill_pending(stream, pending).await?;
    }
    Ok(pending.drain(..length).collect())
}

async fn fill_pending(stream: &mut TcpStream, pending: &mut Vec<u8>) -> Result<()> {
    let mut buf = [0u8; 8192];
    let read = stream.read(&mut buf).await.context("read chunked body")?;
    if read == 0 {
        bail!("chunked body ended early");
    }
    pending.extend_from_slice(&buf[..read]);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn proxy_forwards_absolute_http_request() {
        let upstream_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let upstream_address = upstream_listener.local_addr().unwrap();
        let upstream = tokio::spawn(async move {
            let (mut stream, _) = upstream_listener.accept().await.unwrap();
            let request = read_web_request(&mut stream).await.unwrap();
            assert_eq!(request.method, "GET");
            assert_eq!(request.target, "/hello?name=ii");
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .await
                .unwrap();
        });

        let proxy_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let proxy_address = proxy_listener.local_addr().unwrap();
        let proxy = tokio::spawn(async move {
            let (stream, _) = proxy_listener.accept().await.unwrap();
            serve_proxy_connection(stream, None).await.unwrap();
        });

        let mut client = TcpStream::connect(proxy_address).await.unwrap();
        client
            .write_all(
                format!(
                    "GET http://{upstream_address}/hello?name=ii HTTP/1.1\r\nHost: ignored\r\n\r\n"
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        let mut response = Vec::new();
        client.read_to_end(&mut response).await.unwrap();
        proxy.await.unwrap();
        upstream.await.unwrap();
        assert!(response.starts_with(b"HTTP/1.1 200 OK"));
        assert!(response.ends_with(b"ok"));
    }

    #[tokio::test]
    async fn wake_sends_standard_magic_packet() {
        let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let port = socket.local_addr().unwrap().port();
        wake(WakeArgs {
            mac: [0, 1, 2, 3, 4, 5],
            broadcast: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port,
        })
        .await
        .unwrap();
        let mut packet = [0u8; 128];
        let (length, _) = socket.recv_from(&mut packet).await.unwrap();
        assert_eq!(length, 102);
        assert_eq!(&packet[..6], &[0xff; 6]);
        assert_eq!(&packet[6..12], &[0, 1, 2, 3, 4, 5]);
        assert_eq!(&packet[96..102], &[0, 1, 2, 3, 4, 5]);
    }

    #[tokio::test]
    async fn proxy_connect_tunnels_bytes() {
        let upstream_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let upstream_address = upstream_listener.local_addr().unwrap();
        let upstream = tokio::spawn(async move {
            let (mut stream, _) = upstream_listener.accept().await.unwrap();
            let mut bytes = [0u8; 4];
            stream.read_exact(&mut bytes).await.unwrap();
            assert_eq!(&bytes, b"ping");
            stream.write_all(b"pong").await.unwrap();
        });

        let proxy_listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let proxy_address = proxy_listener.local_addr().unwrap();
        let proxy = tokio::spawn(async move {
            let (stream, _) = proxy_listener.accept().await.unwrap();
            serve_proxy_connection(stream, None).await.unwrap();
        });

        let mut client = TcpStream::connect(proxy_address).await.unwrap();
        client
            .write_all(
                format!("CONNECT {upstream_address} HTTP/1.1\r\nHost: {upstream_address}\r\n\r\n")
                    .as_bytes(),
            )
            .await
            .unwrap();
        let (status, _, pending) = read_http_headers(&mut client).await.unwrap();
        assert_eq!(status, 200);
        assert!(pending.is_empty());
        client.write_all(b"ping").await.unwrap();
        let mut reply = [0u8; 4];
        client.read_exact(&mut reply).await.unwrap();
        assert_eq!(&reply, b"pong");
        drop(client);
        proxy.await.unwrap();
        upstream.await.unwrap();
    }

    #[tokio::test]
    async fn udp_session_uses_target_address_family_and_returns_packets() {
        let upstream = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let target = upstream.local_addr().unwrap();
        let echo = tokio::spawn(async move {
            let mut bytes = [0u8; 64];
            let (length, peer) = upstream.recv_from(&mut bytes).await.unwrap();
            upstream.send_to(&bytes[..length], peer).await.unwrap();
        });
        let listener = Arc::new(UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap());
        let client = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let client_address = client.local_addr().unwrap();
        let (sender, receiver) = mpsc::channel(1);
        let session = tokio::spawn(run_udp_session(
            Arc::clone(&listener),
            client_address,
            target.to_string(),
            receiver,
        ));
        sender.send(b"hello".to_vec()).await.unwrap();
        let mut reply = [0u8; 64];
        let (length, peer) = time::timeout(Duration::from_secs(1), client.recv_from(&mut reply))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(peer, listener.local_addr().unwrap());
        assert_eq!(&reply[..length], b"hello");
        drop(sender);
        session.await.unwrap().unwrap();
        echo.await.unwrap();
    }

    #[test]
    fn response_header_matching_checks_name_and_value() {
        let headers = vec![("transfer-encoding".to_string(), "chunked".to_string())];
        assert!(header_equals(&headers, "transfer-encoding", "chunked"));
        assert!(!header_equals(&headers, "content-type", "chunked"));
    }

    #[test]
    fn http_authority_brackets_ipv6_hosts() {
        assert_eq!(
            http_authority("2001:db8::1", Some(8080)),
            "[2001:db8::1]:8080"
        );
        assert_eq!(http_authority("example.test", None), "example.test");
    }
}
