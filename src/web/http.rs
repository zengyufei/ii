use crate::{
    transport::{progress::fmt_bytes, source::Source},
    web::{
        directory,
        qr::{svg as web_qr_svg, terminal as web_qr_terminal},
        upload,
    },
};
use anyhow::{Context, Result, bail};
use std::{
    io::IsTerminal,
    net::{Ipv4Addr, SocketAddr, UdpSocket},
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::{
    io::{self, AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

pub(crate) enum WebContent {
    Download {
        source: Source,
        download_name: String,
        download_qr_svg: String,
    },
    Directory {
        root: PathBuf,
    },
}

pub(crate) struct WebShare {
    pub(crate) content: WebContent,
    pub(crate) upload_dir: PathBuf,
    pub(crate) web_token: Option<String>,
}

pub(crate) struct WebRequest {
    pub(crate) method: String,
    pub(crate) target: String,
    pub(crate) content_length: Option<u64>,
    pub(crate) range: WebRange,
    pub(crate) body: Vec<u8>,
}

pub(crate) enum WebRange {
    None,
    Header(String),
    Invalid,
}

pub(crate) enum WebFileRange {
    Full,
    Partial { start: u64, end: u64 },
    Unsatisfiable,
}

pub(crate) struct LanWebServer {
    pub(crate) listener: TcpListener,
    pub(crate) url: String,
}

pub(crate) async fn serve_web(
    mut content: WebContent,
    upload_dir: PathBuf,
    web_port: Option<u16>,
    web_token: Option<String>,
) -> Result<()> {
    let lan = start_lan_web_server(web_port, web_token.as_deref(), "ii web").await?;
    if let WebContent::Download {
        download_qr_svg, ..
    } = &mut content
    {
        *download_qr_svg = web_qr_svg(&format!("{}download", lan.url))?;
    }
    let share = Arc::new(WebShare {
        content,
        upload_dir,
        web_token,
    });

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            accepted = lan.listener.accept() => {
                let (stream, _) = accepted.context("accept web connection")?;
                let share = Arc::clone(&share);
                tokio::spawn(async move {
                    if let Err(err) = serve_web_connection(stream, share).await {
                        eprintln!("ii web: request failed: {err:#}");
                    }
                });
            }
        }
    }
    Ok(())
}

pub(crate) async fn start_lan_web_server(
    port: Option<u16>,
    web_token: Option<&str>,
    label: &str,
) -> Result<LanWebServer> {
    let listener = bind_lan_web_listener(port, label).await?;
    let port = listener
        .local_addr()
        .with_context(|| format!("read {label} server address"))?
        .port();
    let host = local_web_host();
    let root_path = web_root_path(web_token);
    let url = format!("http://{host}:{port}{root_path}");

    if std::io::stdout().is_terminal() {
        print!("{}", web_qr_terminal(&url)?);
    }
    println!("{label}: {url}");
    println!();
    println!("other:");
    for host in web_other_hosts(host, web_interface_ipv4_addrs()) {
        println!("http://{host}:{port}{root_path}");
    }
    println!();
    println!("press Ctrl+C to stop sharing");

    Ok(LanWebServer { listener, url })
}

pub(crate) async fn bind_lan_web_listener(port: Option<u16>, label: &str) -> Result<TcpListener> {
    TcpListener::bind((Ipv4Addr::UNSPECIFIED, port.unwrap_or(0)))
        .await
        .with_context(|| format!("bind {label} server"))
}
pub(crate) fn web_upload_dir(start_dir: &Path, configured_dir: Option<&Path>) -> PathBuf {
    match configured_dir {
        Some(dir) if dir.is_absolute() => dir.to_path_buf(),
        Some(dir) => start_dir.join(dir),
        None => start_dir.join("ii"),
    }
}

pub(crate) fn web_root_path(token: Option<&str>) -> String {
    match token {
        Some(token) => format!("/{token}/"),
        None => "/".to_string(),
    }
}

pub(crate) fn local_web_host() -> Ipv4Addr {
    let socket = match UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)) {
        Ok(socket) => socket,
        Err(_) => return Ipv4Addr::LOCALHOST,
    };
    if socket.connect((Ipv4Addr::new(8, 8, 8, 8), 80)).is_ok() {
        if let Ok(SocketAddr::V4(addr)) = socket.local_addr() {
            if !addr.ip().is_unspecified() {
                return *addr.ip();
            }
        }
    }
    Ipv4Addr::LOCALHOST
}

pub(crate) fn web_other_hosts(primary: Ipv4Addr, mut hosts: Vec<Ipv4Addr>) -> Vec<Ipv4Addr> {
    hosts.retain(|host| !host.is_loopback() && !host.is_unspecified() && *host != primary);
    hosts.sort_unstable();
    hosts.dedup();
    hosts
}

#[cfg(windows)]
fn web_interface_ipv4_addrs() -> Vec<Ipv4Addr> {
    use std::{
        ffi::c_void,
        mem,
        ptr::{self, NonNull},
    };

    const AF_INET: u32 = 2;
    const ERROR_BUFFER_OVERFLOW: u32 = 111;

    #[repr(C)]
    union AdapterHeader {
        alignment: u64,
        fields: AdapterHeaderFields,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct AdapterHeaderFields {
        length: u32,
        interface_index: u32,
    }

    #[repr(C)]
    struct AdapterAddresses {
        header: AdapterHeader,
        next: *mut AdapterAddresses,
        adapter_name: *mut i8,
        first_unicast_address: *mut UnicastAddress,
    }

    #[repr(C)]
    struct UnicastAddress {
        header: AdapterHeader,
        next: *mut UnicastAddress,
        address: SocketAddress,
    }

    #[repr(C)]
    struct SocketAddress {
        address: *const c_void,
        length: i32,
    }

    #[repr(C)]
    struct SockAddrIn {
        family: u16,
        port: u16,
        address: [u8; 4],
        zero: [u8; 8],
    }

    #[link(name = "iphlpapi")]
    unsafe extern "system" {
        fn GetAdaptersAddresses(
            family: u32,
            flags: u32,
            reserved: *mut c_void,
            addresses: *mut AdapterAddresses,
            size: *mut u32,
        ) -> u32;
    }

    let mut size = 15 * 1024;
    for _ in 0..2 {
        let words = (size as usize).div_ceil(mem::size_of::<usize>());
        let mut buffer = vec![0usize; words];
        let result = unsafe {
            GetAdaptersAddresses(
                AF_INET,
                0,
                ptr::null_mut(),
                buffer.as_mut_ptr().cast(),
                &mut size,
            )
        };
        if result == ERROR_BUFFER_OVERFLOW {
            continue;
        }
        if result != 0 {
            return Vec::new();
        }

        let mut hosts = Vec::new();
        let mut adapter = NonNull::new(buffer.as_mut_ptr().cast::<AdapterAddresses>());
        while let Some(current) = adapter {
            let mut unicast = unsafe { NonNull::new(current.as_ref().first_unicast_address) };
            while let Some(current) = unicast {
                let address = unsafe { current.as_ref().address.address };
                if !address.is_null() {
                    let address = unsafe { &*address.cast::<SockAddrIn>() };
                    if address.family == AF_INET as u16 {
                        hosts.push(Ipv4Addr::from(address.address));
                    }
                }
                unicast = unsafe { NonNull::new(current.as_ref().next) };
            }
            adapter = unsafe { NonNull::new(current.as_ref().next) };
        }
        return hosts;
    }
    Vec::new()
}

#[cfg(unix)]
fn web_interface_ipv4_addrs() -> Vec<Ipv4Addr> {
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

    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd"
    ))]
    #[repr(C)]
    struct SockAddrIn {
        length: u8,
        family: u8,
        port: u16,
        address: [u8; 4],
        zero: [u8; 8],
    }

    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd"
    )))]
    #[repr(C)]
    struct SockAddrIn {
        family: u16,
        port: u16,
        address: [u8; 4],
        zero: [u8; 8],
    }

    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd"
    ))]
    const AF_INET: u8 = 2;
    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd"
    )))]
    const AF_INET: u16 = 2;

    #[link(name = "c")]
    unsafe extern "C" {
        fn getifaddrs(addresses: *mut *mut IfAddrs) -> c_int;
        fn freeifaddrs(addresses: *mut IfAddrs);
    }

    let mut first = ptr::null_mut();
    if unsafe { getifaddrs(&mut first) } != 0 {
        return Vec::new();
    }

    let mut hosts = Vec::new();
    let mut current = first;
    while !current.is_null() {
        let address = unsafe { (*current).address };
        if !address.is_null() {
            let address = unsafe { &*address.cast::<SockAddrIn>() };
            if address.family == AF_INET {
                hosts.push(Ipv4Addr::from(address.address));
            }
        }
        current = unsafe { (*current).next };
    }
    unsafe { freeifaddrs(first) };
    hosts
}

#[cfg(not(any(windows, unix)))]
fn web_interface_ipv4_addrs() -> Vec<Ipv4Addr> {
    Vec::new()
}

pub(crate) async fn serve_web_connection(
    mut stream: TcpStream,
    share: Arc<WebShare>,
) -> Result<()> {
    let request = match read_web_request(&mut stream).await {
        Ok(request) => request,
        Err(err) => {
            let message = format!("bad request: {err}");
            return write_web_error(&mut stream, "400 Bad Request", &message).await;
        }
    };

    let Some(path) = web_request_path(&share, &request.target) else {
        return write_web_error(&mut stream, "404 Not Found", "not found").await;
    };

    match request.method.as_str() {
        "GET" => match &share.content {
            WebContent::Download {
                source,
                download_name,
                download_qr_svg,
            } => match path {
                "" => write_web_page(&mut stream, source, download_name, download_qr_svg).await,
                "download" => write_web_download(&mut stream, source, download_name).await,
                _ => write_web_error(&mut stream, "404 Not Found", "not found").await,
            },
            WebContent::Directory { root } => {
                directory::write_directory(
                    &mut stream,
                    root,
                    share.web_token.as_deref(),
                    path,
                    &request.target,
                    &request.range,
                    false,
                )
                .await
            }
        },
        "HEAD" => match &share.content {
            WebContent::Directory { root } => {
                directory::write_directory(
                    &mut stream,
                    root,
                    share.web_token.as_deref(),
                    path,
                    &request.target,
                    &request.range,
                    true,
                )
                .await
            }
            WebContent::Download { .. } => {
                write_web_error(&mut stream, "405 Method Not Allowed", "method not allowed").await
            }
        },
        "POST" if path.starts_with("upload?name=") => {
            upload::write_upload(
                &mut stream,
                &share.upload_dir,
                path,
                request.content_length,
                &request.body,
            )
            .await
        }
        "POST" => write_web_error(&mut stream, "404 Not Found", "not found").await,
        _ => write_web_error(&mut stream, "405 Method Not Allowed", "method not allowed").await,
    }
}

fn web_request_path<'a>(share: &WebShare, target: &'a str) -> Option<&'a str> {
    web_token_path(share.web_token.as_deref(), target)
}

pub(crate) fn web_token_path<'a>(web_token: Option<&str>, target: &'a str) -> Option<&'a str> {
    let path = target.strip_prefix('/')?;
    match web_token {
        Some(token) => path.strip_prefix(token)?.strip_prefix('/'),
        None => Some(path),
    }
}

pub(crate) async fn read_web_request(stream: &mut TcpStream) -> Result<WebRequest> {
    let mut request = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    let header_end = loop {
        if let Some(position) = request.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
        if request.len() >= 16 * 1024 {
            bail!("request headers exceed 16 KiB");
        }
        let read = stream.read(&mut chunk).await.context("read web request")?;
        if read == 0 {
            bail!("request ended before headers");
        }
        request.extend_from_slice(&chunk[..read]);
    };

    let headers =
        std::str::from_utf8(&request[..header_end - 4]).context("request is not UTF-8")?;
    let mut lines = headers.split("\r\n");
    let request_line = lines.next().context("request line is missing")?;
    let mut parts = request_line.split_ascii_whitespace();
    let method = parts
        .next()
        .context("request method is missing")?
        .to_string();
    let target = parts
        .next()
        .context("request target is missing")?
        .to_string();
    let _version = parts.next().context("request version is missing")?;
    if parts.next().is_some() {
        bail!("request line is invalid");
    }

    let mut content_length = None;
    let mut range = WebRange::None;
    for line in lines {
        let (name, value) = line.split_once(':').context("request header is invalid")?;
        if name.eq_ignore_ascii_case("content-length") {
            let length = value
                .trim()
                .parse::<u64>()
                .context("Content-Length is invalid")?;
            if content_length.replace(length).is_some() {
                bail!("Content-Length is duplicated");
            }
        } else if name.eq_ignore_ascii_case("range") {
            range = match range {
                WebRange::None => WebRange::Header(value.trim().to_string()),
                WebRange::Header(_) | WebRange::Invalid => WebRange::Invalid,
            };
        }
    }

    Ok(WebRequest {
        method,
        target,
        content_length,
        range,
        body: request.split_off(header_end),
    })
}

async fn write_web_page(
    stream: &mut TcpStream,
    source: &Source,
    download_name: &str,
    download_qr_svg: &str,
) -> Result<()> {
    let name = html_escape(download_name);
    let body = format!(
        "<!doctype html><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{name}</title><style>body{{margin:0;background:#f5f5f5;color:#171717;font-family:system-ui,-apple-system,BlinkMacSystemFont,\"Segoe UI\",sans-serif}}main{{box-sizing:border-box;width:min(100%,32rem);margin:0 auto;padding:2rem 1.25rem 2.5rem;text-align:center}}svg{{display:block;width:min(72vw,17.5rem);height:auto;margin:0 auto 1.5rem;background:#fff}}h1{{margin:0;overflow-wrap:anywhere;font-size:1.5rem;line-height:1.3}}.meta{{margin:0.75rem 0 1.5rem;color:#555;font-size:1rem}}a,button{{box-sizing:border-box;display:block;width:100%;min-height:3rem;padding:0.75rem 1rem;border:0;border-radius:0.25rem;background:#1769aa;color:#fff;font:inherit;font-weight:600;line-height:1.5;text-align:center;text-decoration:none}}button:disabled{{opacity:.6}}.upload{{display:grid;gap:.75rem;margin-top:1.5rem;padding-top:1.5rem;border-top:1px solid #ccc;text-align:left}}input{{box-sizing:border-box;width:100%;min-height:3rem;padding:.625rem;border:1px solid #999;border-radius:.25rem;background:#fff;color:#171717;font:inherit}}output{{display:grid;gap:.375rem;overflow-wrap:anywhere;color:#555;font-size:.875rem;line-height:1.4}}@media (max-width:30rem){{main{{padding:1.5rem 1rem 2rem}}svg{{width:min(82vw,17.5rem);margin-bottom:1.25rem}}h1{{font-size:1.25rem}}.meta{{margin:0.625rem 0 1.25rem}}}}</style><main>{}<h1>{name}</h1><p class=\"meta\">{}</p><a href=\"download\">Download</a><div class=\"upload\"><input id=\"upload\" type=\"file\" multiple aria-label=\"Upload files\"><button id=\"upload-button\" type=\"button\">Upload</button><output id=\"upload-status\" aria-live=\"polite\"></output></div></main><script>const input=document.getElementById('upload');const button=document.getElementById('upload-button');const status=document.getElementById('upload-status');button.addEventListener('click',async()=>{{const files=[...input.files];if(!files.length)return;button.disabled=true;status.textContent='';for(const file of files){{const row=document.createElement('div');row.textContent=file.name;status.append(row);try{{const response=await fetch('upload?name='+encodeURIComponent(file.name),{{method:'POST',body:file}});const text=await response.text();row.textContent=response.ok?text:file.name+': '+text;}}catch(error){{row.textContent=file.name+': '+error;}}}}button.disabled=false;input.value='';}});</script>",
        download_qr_svg,
        fmt_bytes(source.size),
    );
    write_web_response(
        stream,
        "200 OK",
        "text/html; charset=utf-8",
        body.as_bytes(),
    )
    .await
}

async fn write_web_download(
    stream: &mut TcpStream,
    source: &Source,
    download_name: &str,
) -> Result<()> {
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nContent-Disposition: attachment; filename=\"{}\"\r\nConnection: close\r\n\r\n",
        source.size,
        content_disposition_name(download_name),
    );
    stream
        .write_all(header.as_bytes())
        .await
        .context("write download headers")?;
    let mut file = source.open_file().await?;
    io::copy(&mut file, stream)
        .await
        .context("write download")?;
    stream.shutdown().await.context("finish download")?;
    Ok(())
}

pub(crate) async fn write_web_redirect(stream: &mut TcpStream, location: &str) -> Result<()> {
    let header = format!(
        "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
    );
    stream
        .write_all(header.as_bytes())
        .await
        .context("write web redirect")?;
    stream.shutdown().await.context("finish web redirect")?;
    Ok(())
}

pub(crate) async fn write_web_response(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    body: &[u8],
) -> Result<()> {
    write_web_response_with_headers(stream, status, content_type, "", body).await
}

pub(crate) async fn write_web_response_for_method(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    headers: &str,
    body: &[u8],
    head: bool,
) -> Result<()> {
    if head {
        let header = format!(
            "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n",
            body.len(),
        );
        stream
            .write_all(header.as_bytes())
            .await
            .context("write web headers")?;
        stream.shutdown().await.context("finish web response")?;
        Ok(())
    } else {
        write_web_response_with_headers(stream, status, content_type, headers, body).await
    }
}

pub(crate) async fn write_web_response_with_headers(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    headers: &str,
    body: &[u8],
) -> Result<()> {
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n",
        body.len(),
    );
    stream
        .write_all(header.as_bytes())
        .await
        .context("write web headers")?;
    stream.write_all(body).await.context("write web body")?;
    stream.shutdown().await.context("finish web response")?;
    Ok(())
}

pub(crate) async fn write_web_error(
    stream: &mut TcpStream,
    status: &str,
    message: &str,
) -> Result<()> {
    write_web_response(
        stream,
        status,
        "text/plain; charset=utf-8",
        message.as_bytes(),
    )
    .await
}

fn content_disposition_name(name: &str) -> String {
    name.replace(['\r', '\n', '\\', '"'], "_")
}

pub(crate) fn html_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
