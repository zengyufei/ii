use crate::{
    command::{DropArgs, HttpArgs, LanHttpArgs, PacArgs, PasteArgs},
    discovery::{self, Service},
    web::{
        directory,
        http::{
            WebRequest, html_escape, read_web_request, start_lan_web_server, web_token_path,
            write_web_error, write_web_response_for_method, write_web_response_with_headers,
        },
        upload,
    },
};
use anyhow::{Context, Result, bail};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time,
};

const SPEED_CHUNK: usize = 1024 * 1024;
const SPEED_MAX_DURATION: Duration = Duration::from_secs(60 * 60);

#[derive(Clone)]
pub(super) enum LanContent {
    Directory(PathBuf),
    Paste(String),
    Drop(PathBuf),
    Pac(String),
    Speed,
}

struct LanState {
    content: LanContent,
    token: Option<String>,
    sessions: upload::UploadSessions,
}

pub(super) async fn serve(
    content: LanContent,
    listen: LanHttpArgs,
    label: &str,
    kind: &str,
    ttl: Option<Duration>,
) -> Result<()> {
    let lan =
        start_lan_web_server(listen.port, listen.bind, listen.token.as_deref(), label).await?;
    let _advertiser = discovery::advertise(Service::Http {
        kind: kind.to_string(),
        url: lan.url,
    })
    .await?;
    let state = Arc::new(LanState {
        content,
        token: listen.token,
        sessions: upload::sessions(),
    });
    serve_listener(lan.listener, state, ttl).await
}

pub(super) async fn http(args: HttpArgs) -> Result<()> {
    let start = std::env::current_dir().context("read current directory for HTTP service")?;
    let root = directory::directory_root(&start, args.dir.as_deref()).await?;
    serve(
        LanContent::Directory(root),
        args.listen,
        "ii http",
        "http",
        None,
    )
    .await
}

pub(super) async fn paste(args: PasteArgs) -> Result<()> {
    let text = match args.text {
        Some(text) => text,
        None => {
            let mut text = String::new();
            tokio::io::stdin()
                .read_to_string(&mut text)
                .await
                .context("read paste text from stdin")?;
            text
        }
    };
    serve(
        LanContent::Paste(text),
        args.listen,
        "ii paste",
        "paste",
        args.ttl,
    )
    .await
}

pub(super) async fn drop(args: DropArgs) -> Result<()> {
    let start = std::env::current_dir().context("read current directory for drop service")?;
    let target = match args.dir {
        Some(path) if path.is_absolute() => path,
        Some(path) => start.join(path),
        None => start.join("ii"),
    };
    serve(
        LanContent::Drop(target),
        args.listen,
        "ii drop",
        "drop",
        None,
    )
    .await
}

pub(super) async fn pac(args: PacArgs) -> Result<()> {
    let script = pac_script(&args.proxy)?;
    serve(LanContent::Pac(script), args.listen, "ii pac", "pac", None).await
}

pub(super) async fn speed_server(listen: LanHttpArgs) -> Result<()> {
    serve(LanContent::Speed, listen, "ii speed", "speed", None).await
}

async fn serve_listener(
    listener: TcpListener,
    state: Arc<LanState>,
    ttl: Option<Duration>,
) -> Result<()> {
    let mut cleanup_tick = time::interval(Duration::from_secs(60));
    let deadline = ttl.map(|ttl| time::Instant::now() + ttl);
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            _ = wait_deadline(deadline), if deadline.is_some() => break,
            _ = cleanup_tick.tick() => upload::cleanup_expired(&state.sessions).await,
            accepted = listener.accept() => {
                let (stream, _) = accepted.context("accept LAN HTTP connection")?;
                let state = Arc::clone(&state);
                tokio::spawn(async move {
                    if let Err(err) = serve_connection(stream, state).await {
                        eprintln!("ii LAN HTTP: request failed: {err:#}");
                    }
                });
            }
        }
    }
    upload::cleanup_all(&state.sessions).await;
    Ok(())
}

async fn wait_deadline(deadline: Option<time::Instant>) {
    match deadline {
        Some(deadline) => time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

async fn serve_connection(mut stream: TcpStream, state: Arc<LanState>) -> Result<()> {
    let request = match read_web_request(&mut stream).await {
        Ok(request) => request,
        Err(err) => {
            return write_web_error(
                &mut stream,
                "400 Bad Request",
                &format!("bad request: {err}"),
            )
            .await;
        }
    };
    let Some(path) = web_token_path(state.token.as_deref(), &request.target) else {
        return write_web_error(&mut stream, "404 Not Found", "not found").await;
    };

    if matches!(state.content, LanContent::Drop(_))
        && handle_upload(&mut stream, &request, path, &state).await?
    {
        return Ok(());
    }

    match &state.content {
        LanContent::Directory(root) => match request.method.as_str() {
            "GET" => {
                directory::write_directory(
                    &mut stream,
                    root,
                    state.token.as_deref(),
                    path,
                    &request.target,
                    &request.range,
                    false,
                    false,
                )
                .await
            }
            "HEAD" => {
                directory::write_directory(
                    &mut stream,
                    root,
                    state.token.as_deref(),
                    path,
                    &request.target,
                    &request.range,
                    true,
                    false,
                )
                .await
            }
            _ => write_web_error(&mut stream, "405 Method Not Allowed", "method not allowed").await,
        },
        LanContent::Paste(text) => serve_paste(&mut stream, &request, path, text).await,
        LanContent::Drop(_) => serve_drop(&mut stream, &request, path).await,
        LanContent::Pac(script) => serve_pac(&mut stream, &request, path, script).await,
        LanContent::Speed => serve_speed(&mut stream, &request, path).await,
    }
}

async fn handle_upload(
    stream: &mut TcpStream,
    request: &WebRequest,
    path: &str,
    state: &LanState,
) -> Result<bool> {
    let LanContent::Drop(upload_dir) = &state.content else {
        return Ok(false);
    };
    match request.method.as_str() {
        "HEAD" if path.starts_with("upload?name=") && path.contains("&upload=") => {
            upload::session_head(stream, path, &state.sessions).await?;
            Ok(true)
        }
        "POST" if path.starts_with("upload/init?") => {
            upload::create_session(stream, upload_dir, path, &state.sessions).await?;
            Ok(true)
        }
        "PATCH" if path.starts_with("upload?name=") && path.contains("&upload=") => {
            upload::write_upload_chunk(
                stream,
                upload_dir,
                "ii drop",
                path,
                request.content_length,
                request.header("content-range"),
                &request.body,
                &state.sessions,
            )
            .await?;
            Ok(true)
        }
        "POST" if path.starts_with("upload?name=") && !path.contains("&upload=") => {
            upload::write_upload(
                stream,
                upload_dir,
                "ii drop",
                path,
                request.content_length,
                &request.body,
            )
            .await?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

async fn serve_paste(
    stream: &mut TcpStream,
    request: &WebRequest,
    path: &str,
    text: &str,
) -> Result<()> {
    match (request.method.as_str(), path) {
        ("GET" | "HEAD", "") => {
            let body = format!(
                "<!doctype html><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>ii paste</title><style>body{{margin:0;background:#fff;color:#111;font-family:ui-monospace,Consolas,monospace}}main{{box-sizing:border-box;max-width:52rem;margin:auto;padding:1.25rem}}h1{{font-size:1.25rem}}pre{{white-space:pre-wrap;overflow-wrap:anywhere;border:1px solid #bbb;padding:1rem}}button,a{{font:inherit;padding:.5rem .75rem}}a{{margin-left:.5rem}}</style><main><h1>Paste</h1><button onclick=\"navigator.clipboard.writeText(document.querySelector('pre').textContent)\">Copy</button><a href=\"raw\">Raw</a><pre>{}</pre></main>",
                html_escape(text)
            );
            write_web_response_for_method(
                stream,
                "200 OK",
                "text/html; charset=utf-8",
                "",
                body.as_bytes(),
                request.method == "HEAD",
            )
            .await
        }
        ("GET" | "HEAD", "raw") => {
            write_web_response_for_method(
                stream,
                "200 OK",
                "text/plain; charset=utf-8",
                "",
                text.as_bytes(),
                request.method == "HEAD",
            )
            .await
        }
        ("GET" | "HEAD", _) => write_web_error(stream, "404 Not Found", "not found").await,
        _ => write_web_error(stream, "405 Method Not Allowed", "method not allowed").await,
    }
}

async fn serve_drop(stream: &mut TcpStream, request: &WebRequest, path: &str) -> Result<()> {
    if !matches!(request.method.as_str(), "GET" | "HEAD") {
        return write_web_error(stream, "405 Method Not Allowed", "method not allowed").await;
    }
    if !path.is_empty() {
        return write_web_error(stream, "404 Not Found", "not found").await;
    }
    let body = format!(
        "<!doctype html><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>ii drop</title><style>body{{margin:0;background:#fff;color:#111;font-family:system-ui,sans-serif}}main{{box-sizing:border-box;width:min(100%,34rem);margin:auto;padding:1.5rem 1rem}}h1{{font-size:1.4rem}}</style><main><h1>Upload files</h1>{}</main>",
        upload::html_controls()
    );
    write_web_response_for_method(
        stream,
        "200 OK",
        "text/html; charset=utf-8",
        "",
        body.as_bytes(),
        request.method == "HEAD",
    )
    .await
}

async fn serve_pac(
    stream: &mut TcpStream,
    request: &WebRequest,
    path: &str,
    script: &str,
) -> Result<()> {
    if !matches!(request.method.as_str(), "GET" | "HEAD") {
        return write_web_error(stream, "405 Method Not Allowed", "method not allowed").await;
    }
    if !path.is_empty() {
        return write_web_error(stream, "404 Not Found", "not found").await;
    }
    write_web_response_for_method(
        stream,
        "200 OK",
        "application/x-ns-proxy-autoconfig; charset=utf-8",
        "",
        script.as_bytes(),
        request.method == "HEAD",
    )
    .await
}

async fn serve_speed(stream: &mut TcpStream, request: &WebRequest, path: &str) -> Result<()> {
    match (request.method.as_str(), path) {
        ("GET" | "HEAD", "") => {
            let body = b"ii speed server\n";
            write_web_response_for_method(
                stream,
                "200 OK",
                "text/plain; charset=utf-8",
                "",
                body,
                request.method == "HEAD",
            )
            .await
        }
        ("GET", path) if path.starts_with("download?") => {
            let duration = speed_duration(path)?;
            write_speed_download(stream, duration).await
        }
        ("POST", path) if path.starts_with("upload?") => {
            let duration = speed_duration(path)?;
            if !request
                .header("transfer-encoding")
                .is_some_and(|value| value.eq_ignore_ascii_case("chunked"))
            {
                return write_web_error(stream, "400 Bad Request", "chunked upload is required")
                    .await;
            }
            let bytes = read_chunked_body(stream, request.body.clone()).await?;
            let body = format!("{bytes}\n");
            let _ = duration;
            write_web_response_with_headers(
                stream,
                "200 OK",
                "text/plain; charset=utf-8",
                "",
                body.as_bytes(),
            )
            .await
        }
        ("GET" | "HEAD", _) => write_web_error(stream, "404 Not Found", "not found").await,
        _ => write_web_error(stream, "405 Method Not Allowed", "method not allowed").await,
    }
}

fn speed_duration(path: &str) -> Result<Duration> {
    let value = path
        .split_once('?')
        .and_then(|(_, query)| {
            query
                .split('&')
                .find_map(|part| part.strip_prefix("duration_ms="))
        })
        .context("speed duration is missing")?;
    let milliseconds = value.parse::<u64>().context("speed duration is invalid")?;
    let duration = Duration::from_millis(milliseconds);
    if duration.is_zero() || duration > SPEED_MAX_DURATION {
        bail!("speed duration is invalid");
    }
    Ok(duration)
}

async fn write_speed_download(stream: &mut TcpStream, duration: Duration) -> Result<()> {
    stream
        .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n")
        .await
        .context("write speed download headers")?;
    let deadline = time::Instant::now() + duration;
    let bytes = vec![0u8; SPEED_CHUNK];
    while time::Instant::now() < deadline {
        stream
            .write_all(format!("{:X}\r\n", bytes.len()).as_bytes())
            .await
            .context("write speed chunk header")?;
        stream
            .write_all(&bytes)
            .await
            .context("write speed chunk")?;
        stream
            .write_all(b"\r\n")
            .await
            .context("finish speed chunk")?;
    }
    stream
        .write_all(b"0\r\n\r\n")
        .await
        .context("finish speed download")?;
    stream.shutdown().await.context("close speed download")?;
    Ok(())
}

async fn read_chunked_body(stream: &mut TcpStream, mut pending: Vec<u8>) -> Result<u64> {
    let mut total = 0u64;
    loop {
        let line = read_chunk_line(stream, &mut pending).await?;
        let length = std::str::from_utf8(&line)
            .context("chunk length is not UTF-8")?
            .split(';')
            .next()
            .context("chunk length is missing")?;
        let length = u64::from_str_radix(length, 16).context("chunk length is invalid")?;
        if length == 0 {
            loop {
                if read_chunk_line(stream, &mut pending).await?.is_empty() {
                    return Ok(total);
                }
            }
        }
        discard_chunk(stream, &mut pending, length).await?;
        let ending = read_exact_buffered(stream, &mut pending, 2).await?;
        if ending != b"\r\n" {
            bail!("chunk body is invalid");
        }
        total = total
            .checked_add(length)
            .context("speed upload is too large")?;
    }
}

async fn read_chunk_line(stream: &mut TcpStream, pending: &mut Vec<u8>) -> Result<Vec<u8>> {
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

async fn discard_chunk(stream: &mut TcpStream, pending: &mut Vec<u8>, length: u64) -> Result<()> {
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

pub(super) fn pac_script(proxy: &str) -> Result<String> {
    let url = url::Url::parse(proxy).context("parse PAC proxy URL")?;
    let host = url.host_str().context("PAC proxy host is missing")?;
    let port = url.port().context("PAC proxy port is missing")?;
    let endpoint = if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    };
    let directive = if url.scheme() == "socks5" {
        "SOCKS5"
    } else {
        "PROXY"
    };
    Ok(format!(
        "function FindProxyForURL(url, host) {{\n  host = host.toLowerCase();\n  if (host == 'localhost' || isPlainHostName(host) || shExpMatch(host, '*.local') || host == '::1' || shExpMatch(host, 'fe80:*') || isInNet(host, '127.0.0.0', '255.0.0.0') || isInNet(host, '10.0.0.0', '255.0.0.0') || isInNet(host, '172.16.0.0', '255.240.0.0') || isInNet(host, '192.168.0.0', '255.255.0.0') || isInNet(host, '169.254.0.0', '255.255.0.0')) return 'DIRECT';\n  return '{directive} {endpoint}';\n}}\n"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn request(content: LanContent, token: Option<&str>, request: &[u8]) -> Vec<u8> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let state = Arc::new(LanState {
            content,
            token: token.map(str::to_owned),
            sessions: upload::sessions(),
        });
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            serve_connection(stream, state).await.unwrap();
        });
        let mut stream = TcpStream::connect(address).await.unwrap();
        stream.write_all(request).await.unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.unwrap();
        server.await.unwrap();
        response
    }

    #[tokio::test]
    async fn paste_honors_token_and_serves_raw_text() {
        let token = "A1b2C3d4E5f6G7h8";
        let response = request(
            LanContent::Paste("line one\nline two".to_string()),
            Some(token),
            format!("GET /{token}/raw HTTP/1.1\r\nHost: test\r\n\r\n").as_bytes(),
        )
        .await;
        assert!(response.starts_with(b"HTTP/1.1 200 OK"));
        assert!(
            response
                .windows(b"line one\nline two".len())
                .any(|window| window == b"line one\nline two")
        );

        let response = request(
            LanContent::Paste("text".to_string()),
            Some(token),
            b"GET /raw HTTP/1.1\r\nHost: test\r\n\r\n",
        )
        .await;
        assert!(response.starts_with(b"HTTP/1.1 404 Not Found"));
    }

    #[tokio::test]
    async fn drop_page_and_pac_response_are_specialized() {
        let response = request(
            LanContent::Drop(PathBuf::from("uploads")),
            None,
            b"GET / HTTP/1.1\r\nHost: test\r\n\r\n",
        )
        .await;
        assert!(response.starts_with(b"HTTP/1.1 200 OK"));
        assert!(
            response
                .windows(b"multiple".len())
                .any(|window| window == b"multiple")
        );
        assert!(
            !response
                .windows(b"Index of".len())
                .any(|window| window == b"Index of")
        );

        let script = pac_script("socks5://127.0.0.1:1080").unwrap();
        let response = request(
            LanContent::Pac(script),
            None,
            b"GET / HTTP/1.1\r\nHost: test\r\n\r\n",
        )
        .await;
        assert!(response.starts_with(b"HTTP/1.1 200 OK"));
        assert!(
            response
                .windows(b"application/x-ns-proxy-autoconfig".len())
                .any(|window| window == b"application/x-ns-proxy-autoconfig")
        );
        assert!(
            response
                .windows(b"SOCKS5 127.0.0.1:1080".len())
                .any(|window| window == b"SOCKS5 127.0.0.1:1080")
        );
    }

    #[tokio::test]
    async fn speed_accepts_chunked_upload() {
        let response = request(
            LanContent::Speed,
            None,
            b"POST /upload?duration_ms=1 HTTP/1.1\r\nHost: test\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n",
        )
        .await;
        assert!(response.starts_with(b"HTTP/1.1 200 OK"));
        assert!(response.ends_with(b"5\n"));
    }

    #[tokio::test]
    async fn drop_writes_uploaded_file() {
        let uploads = tempfile::tempdir().unwrap();
        let response = request(
            LanContent::Drop(uploads.path().to_path_buf()),
            None,
            b"POST /upload?name=note.txt HTTP/1.1\r\nHost: test\r\nContent-Length: 5\r\n\r\nhello",
        )
        .await;
        assert!(response.starts_with(b"HTTP/1.1 201 Created"));
        assert_eq!(
            tokio::fs::read(uploads.path().join("note.txt"))
                .await
                .unwrap(),
            b"hello"
        );
    }
}
