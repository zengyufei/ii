use crate::{
    discovery::{self, Service},
    web::{
        directory,
        http::{
            WebRequest, html_escape, read_web_request, start_lan_web_server, web_root_path,
            web_token_path, write_web_error, write_web_response, write_web_response_with_headers,
        },
    },
};
use anyhow::{Context, Result, bail};
use percent_encoding::{NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};
use rand::RngExt;
use std::{
    collections::HashMap,
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tempfile::NamedTempFile;
use time::{
    OffsetDateTime,
    format_description::{FormatItem, well_known::Rfc3339},
    macros::format_description,
};

const HTTP_DATE_FORMAT: &[FormatItem<'static>] = format_description!(
    "[weekday repr:short], [day padding:zero] [month repr:short] [year repr:full] [hour]:[minute]:[second] GMT"
);
use tokio::{
    fs,
    io::{self, AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

struct Lock {
    token: String,
    expires: std::time::Instant,
}

#[derive(Default)]
struct LockState {
    locks: HashMap<PathBuf, Lock>,
}

pub(crate) async fn serve_dav(
    root: PathBuf,
    port: Option<u16>,
    bind: Option<std::net::IpAddr>,
    token: Option<String>,
    read_only: bool,
) -> Result<()> {
    let lan = start_lan_web_server(port, bind, token.as_deref(), "ii dav").await?;
    let _advertiser = discovery::advertise(Service::Dav {
        url: lan.url.clone(),
    })
    .await?;
    let locks = Arc::new(Mutex::new(LockState::default()));
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            accepted = lan.listener.accept() => {
                let (stream, _) = accepted.context("accept DAV connection")?;
                let root = root.clone();
                let token = token.clone();
                let locks = Arc::clone(&locks);
                tokio::spawn(async move {
                    if let Err(err) = serve_dav_connection(stream, root, token, read_only, locks).await {
                        eprintln!("ii dav: request failed: {err:#}");
                    }
                });
            }
        }
    }
    Ok(())
}

async fn serve_dav_connection(
    mut stream: TcpStream,
    root: PathBuf,
    token: Option<String>,
    read_only: bool,
    locks: Arc<Mutex<LockState>>,
) -> Result<()> {
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
    let request_path = request.target.split('?').next().unwrap_or(&request.target);
    let Some(path) = web_token_path(token.as_deref(), request_path) else {
        return write_web_error(&mut stream, "404 Not Found", "not found").await;
    };
    let relative = match dav_relative_path(path) {
        Ok(path) => path,
        Err(_) => return write_web_error(&mut stream, "404 Not Found", "not found").await,
    };
    match request.method.as_str() {
        "OPTIONS" => write_options(&mut stream, read_only).await,
        "PROPFIND" => propfind(&mut stream, &root, &relative, token.as_deref(), &request).await,
        "GET" | "HEAD" => {
            directory::write_directory(
                &mut stream,
                &root,
                token.as_deref(),
                path,
                request_path,
                &request.range,
                request.method == "HEAD",
                false,
            )
            .await
        }
        "PUT" if read_only => {
            write_web_error(&mut stream, "403 Forbidden", "DAV is read-only").await
        }
        "PUT" => put(&mut stream, &root, &relative, &request, &locks).await,
        "MKCOL" if read_only => {
            write_web_error(&mut stream, "403 Forbidden", "DAV is read-only").await
        }
        "MKCOL" => mkcol(&mut stream, &root, &relative, &request, &locks).await,
        "DELETE" if read_only => {
            write_web_error(&mut stream, "403 Forbidden", "DAV is read-only").await
        }
        "DELETE" => delete(&mut stream, &root, &relative, &request, &locks).await,
        "MOVE" | "COPY" if read_only => {
            write_web_error(&mut stream, "403 Forbidden", "DAV is read-only").await
        }
        "MOVE" | "COPY" => {
            move_or_copy(
                &mut stream,
                &root,
                &relative,
                token.as_deref(),
                &request,
                &locks,
                request.method == "MOVE",
            )
            .await
        }
        "LOCK" if read_only => {
            write_web_error(&mut stream, "403 Forbidden", "DAV is read-only").await
        }
        "LOCK" => {
            lock(
                &mut stream,
                &root,
                &relative,
                token.as_deref(),
                &request,
                &locks,
            )
            .await
        }
        "UNLOCK" if read_only => {
            write_web_error(&mut stream, "403 Forbidden", "DAV is read-only").await
        }
        "UNLOCK" => unlock(&mut stream, &root, &relative, &request, &locks).await,
        "PROPPATCH" => proppatch(&mut stream, token.as_deref(), &relative).await,
        _ => write_web_error(&mut stream, "405 Method Not Allowed", "method not allowed").await,
    }
}

async fn write_options(stream: &mut TcpStream, read_only: bool) -> Result<()> {
    let allow = if read_only {
        "OPTIONS, PROPFIND, GET, HEAD, PROPPATCH"
    } else {
        "OPTIONS, PROPFIND, GET, HEAD, PUT, MKCOL, DELETE, MOVE, COPY, LOCK, UNLOCK, PROPPATCH"
    };
    write_web_response_with_headers(
        stream,
        "200 OK",
        "text/plain; charset=utf-8",
        &format!("DAV: 1, 2\r\nAllow: {allow}\r\nMS-Author-Via: DAV\r\n"),
        b"",
    )
    .await
}

async fn propfind(
    stream: &mut TcpStream,
    root: &Path,
    relative: &Path,
    token: Option<&str>,
    request: &WebRequest,
) -> Result<()> {
    let depth = match request.header("depth").unwrap_or("infinity") {
        "0" => 0,
        "1" => 1,
        _ => return write_web_error(stream, "403 Forbidden", "Depth must be 0 or 1").await,
    };
    let target = match existing_target(root, relative).await {
        Ok(target) => target,
        Err(_) => return write_web_error(stream, "404 Not Found", "not found").await,
    };
    let mut entries = vec![target.clone()];
    if depth == 1 && fs::metadata(&target).await?.is_dir() {
        let mut children = fs::read_dir(&target).await?;
        while let Some(entry) = children.next_entry().await? {
            entries.push(entry.path());
        }
    }
    let mut body =
        String::from("<?xml version=\"1.0\" encoding=\"utf-8\"?><d:multistatus xmlns:d=\"DAV:\">");
    for entry in entries {
        body.push_str(&prop_response(root, &entry, token).await?);
    }
    body.push_str("</d:multistatus>");
    write_web_response(
        stream,
        "207 Multi-Status",
        "application/xml; charset=utf-8",
        body.as_bytes(),
    )
    .await
}

async fn put(
    stream: &mut TcpStream,
    root: &Path,
    relative: &Path,
    request: &WebRequest,
    locks: &Arc<Mutex<LockState>>,
) -> Result<()> {
    let target = match writable_target(root, relative).await {
        Ok(target) => target,
        Err(_) => {
            return write_web_error(stream, "409 Conflict", "parent directory is missing").await;
        }
    };
    if !lock_permits(locks, &target, request) {
        return write_web_error(stream, "423 Locked", "resource is locked").await;
    }
    if request
        .header("expect")
        .is_some_and(|value| value.eq_ignore_ascii_case("100-continue"))
    {
        stream.write_all(b"HTTP/1.1 100 Continue\r\n\r\n").await?;
    }
    let parent = target.parent().context("DAV target has no parent")?;
    let temp = NamedTempFile::new_in(parent).context("create DAV temporary file")?;
    let mut file = fs::File::from_std(temp.reopen().context("open DAV temporary file")?);
    let write_result = write_request_body(stream, request, &mut file).await;
    if let Err(err) = write_result {
        return write_web_error(stream, "400 Bad Request", &format!("write DAV body: {err}")).await;
    }
    file.flush().await.context("flush DAV file")?;
    drop(file);
    if let Ok(metadata) = fs::metadata(&target).await {
        if metadata.is_dir() {
            return write_web_error(stream, "409 Conflict", "target is a directory").await;
        }
    }
    temp.persist(&target)
        .map_err(|err| anyhow::anyhow!("replace DAV file: {}", err.error))?;
    write_web_response(stream, "201 Created", "text/plain; charset=utf-8", b"").await
}

async fn mkcol(
    stream: &mut TcpStream,
    root: &Path,
    relative: &Path,
    request: &WebRequest,
    locks: &Arc<Mutex<LockState>>,
) -> Result<()> {
    if relative.as_os_str().is_empty() {
        return write_web_error(stream, "405 Method Not Allowed", "cannot create root").await;
    }
    if !request.body.is_empty() || request.content_length.unwrap_or(0) > 0 {
        return write_web_error(
            stream,
            "415 Unsupported Media Type",
            "MKCOL body is unsupported",
        )
        .await;
    }
    let target = match writable_target(root, relative).await {
        Ok(target) => target,
        Err(_) => {
            return write_web_error(stream, "409 Conflict", "parent directory is missing").await;
        }
    };
    if !lock_permits(locks, &target, request) {
        return write_web_error(stream, "423 Locked", "resource is locked").await;
    }
    match fs::create_dir(&target).await {
        Ok(()) => write_web_response(stream, "201 Created", "text/plain; charset=utf-8", b"").await,
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
            write_web_error(stream, "405 Method Not Allowed", "resource already exists").await
        }
        Err(err) => Err(err).context("create DAV collection"),
    }
}

async fn delete(
    stream: &mut TcpStream,
    root: &Path,
    relative: &Path,
    request: &WebRequest,
    locks: &Arc<Mutex<LockState>>,
) -> Result<()> {
    if relative.as_os_str().is_empty() {
        return write_web_error(stream, "403 Forbidden", "cannot delete root").await;
    }
    let target = match existing_target(root, relative).await {
        Ok(target) => target,
        Err(_) => return write_web_error(stream, "404 Not Found", "not found").await,
    };
    if !lock_permits(locks, &target, request) {
        return write_web_error(stream, "423 Locked", "resource is locked").await;
    }
    if fs::metadata(&target).await?.is_dir() {
        fs::remove_dir_all(&target)
            .await
            .context("delete DAV collection")?;
    } else {
        fs::remove_file(&target).await.context("delete DAV file")?;
    }
    write_web_response(stream, "204 No Content", "text/plain; charset=utf-8", b"").await
}

async fn move_or_copy(
    stream: &mut TcpStream,
    root: &Path,
    relative: &Path,
    token: Option<&str>,
    request: &WebRequest,
    locks: &Arc<Mutex<LockState>>,
    move_file: bool,
) -> Result<()> {
    let source = match existing_target(root, relative).await {
        Ok(source) if !relative.as_os_str().is_empty() => source,
        Ok(_) => return write_web_error(stream, "403 Forbidden", "cannot move root").await,
        Err(_) => return write_web_error(stream, "404 Not Found", "not found").await,
    };
    let Some(destination) = request.header("destination") else {
        return write_web_error(stream, "400 Bad Request", "Destination is required").await;
    };
    let destination = match destination_relative(destination, token).and_then(|path| {
        if path.as_os_str().is_empty() {
            bail!("root")
        } else {
            Ok(path)
        }
    }) {
        Ok(path) => match writable_target(root, &path).await {
            Ok(path) => path,
            Err(_) => {
                return write_web_error(stream, "409 Conflict", "destination parent is missing")
                    .await;
            }
        },
        Err(_) => return write_web_error(stream, "400 Bad Request", "invalid Destination").await,
    };
    if !lock_permits(locks, &source, request) || !lock_permits(locks, &destination, request) {
        return write_web_error(stream, "423 Locked", "resource is locked").await;
    }
    if fs::metadata(&source).await?.is_dir() && destination.starts_with(&source) {
        return write_web_error(stream, "403 Forbidden", "destination is inside source").await;
    }
    let exists = fs::metadata(&destination).await.is_ok();
    if exists
        && request
            .header("overwrite")
            .is_some_and(|value| value.eq_ignore_ascii_case("F"))
    {
        return write_web_error(stream, "412 Precondition Failed", "destination exists").await;
    }
    if exists {
        if fs::metadata(&destination).await?.is_dir() {
            fs::remove_dir_all(&destination).await?;
        } else {
            fs::remove_file(&destination).await?;
        }
    }
    if move_file {
        fs::rename(&source, &destination)
            .await
            .context("move DAV resource")?;
        move_locks(locks, &source, &destination);
    } else {
        tokio::task::spawn_blocking(move || copy_path(&source, &destination))
            .await
            .context("copy DAV task")??;
    }
    write_web_response(
        stream,
        if exists {
            "204 No Content"
        } else {
            "201 Created"
        },
        "text/plain; charset=utf-8",
        b"",
    )
    .await
}

async fn lock(
    stream: &mut TcpStream,
    root: &Path,
    relative: &Path,
    token: Option<&str>,
    request: &WebRequest,
    locks: &Arc<Mutex<LockState>>,
) -> Result<()> {
    let existed = existing_target(root, relative).await.is_ok();
    let target = match writable_target(root, relative).await {
        Ok(target) => target,
        Err(_) => {
            return write_web_error(stream, "409 Conflict", "parent directory is missing").await;
        }
    };
    let timeout = request
        .header("timeout")
        .and_then(|value| value.strip_prefix("Second-"))
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(3600)
        .clamp(1, 3600);
    let token_value = {
        let mut state = locks.lock().expect("DAV lock mutex poisoned");
        prune_locks(&mut state);
        if let Some(lock) = state.locks.get_mut(&target) {
            if !request_presents_lock(lock, request) {
                None
            } else {
                lock.expires = std::time::Instant::now() + Duration::from_secs(timeout);
                Some(lock.token.clone())
            }
        } else if state
            .locks
            .iter()
            .any(|(path, _)| target.starts_with(path) || path.starts_with(&target))
        {
            None
        } else {
            let token = format!("opaquelocktoken:{:032x}", rand::rng().random::<u128>());
            state.locks.insert(
                target.clone(),
                Lock {
                    token: token.clone(),
                    expires: std::time::Instant::now() + Duration::from_secs(timeout),
                },
            );
            Some(token)
        }
    };
    let Some(token_value) = token_value else {
        return write_web_error(stream, "423 Locked", "resource is locked").await;
    };
    let href = dav_href(root, &target, token)?;
    let body = format!(
        "<?xml version=\"1.0\"?><d:prop xmlns:d=\"DAV:\"><d:lockdiscovery><d:activelock><d:locktype><d:write/></d:locktype><d:lockscope><d:exclusive/></d:lockscope><d:depth>infinity</d:depth><d:timeout>Second-{timeout}</d:timeout><d:locktoken><d:href>{}</d:href></d:locktoken><d:lockroot><d:href>{href}</d:href></d:lockroot></d:activelock></d:lockdiscovery></d:prop>",
        xml_escape(&token_value)
    );
    write_web_response_with_headers(
        stream,
        if existed { "200 OK" } else { "201 Created" },
        "application/xml; charset=utf-8",
        &format!("Lock-Token: <{token_value}>\r\n"),
        body.as_bytes(),
    )
    .await
}

async fn unlock(
    stream: &mut TcpStream,
    root: &Path,
    relative: &Path,
    request: &WebRequest,
    locks: &Arc<Mutex<LockState>>,
) -> Result<()> {
    let target = match existing_or_writable_target(root, relative).await {
        Ok(target) => target,
        Err(_) => {
            return write_web_error(stream, "409 Conflict", "lock target is unavailable").await;
        }
    };
    let token = request
        .header("lock-token")
        .map(strip_lock_token)
        .unwrap_or_default();
    let removed = {
        let mut state = locks.lock().expect("DAV lock mutex poisoned");
        prune_locks(&mut state);
        matches!(state.locks.get(&target), Some(lock) if lock.token == token)
            && state.locks.remove(&target).is_some()
    };
    if removed {
        write_web_response(stream, "204 No Content", "text/plain; charset=utf-8", b"").await
    } else {
        write_web_error(stream, "409 Conflict", "lock token does not match").await
    }
}

async fn proppatch(stream: &mut TcpStream, token: Option<&str>, relative: &Path) -> Result<()> {
    let href = dav_relative_href(relative, token);
    let body = format!(
        "<?xml version=\"1.0\"?><d:multistatus xmlns:d=\"DAV:\"><d:response><d:href>{href}</d:href><d:propstat><d:prop/><d:status>HTTP/1.1 403 Forbidden</d:status></d:propstat></d:response></d:multistatus>"
    );
    write_web_response(
        stream,
        "207 Multi-Status",
        "application/xml; charset=utf-8",
        body.as_bytes(),
    )
    .await
}

async fn write_request_body(
    stream: &mut TcpStream,
    request: &WebRequest,
    file: &mut fs::File,
) -> Result<()> {
    if request
        .header("transfer-encoding")
        .is_some_and(|value| value.eq_ignore_ascii_case("chunked"))
    {
        return write_chunked_body(stream, request.body.clone(), file).await;
    }
    let length = request
        .content_length
        .context("Content-Length is required")?;
    let initial = u64::try_from(request.body.len()).context("request body is too large")?;
    if initial > length {
        bail!("body exceeds Content-Length");
    }
    file.write_all(&request.body).await?;
    let remaining = length - initial;
    let mut body = stream.take(remaining);
    let copied = io::copy(&mut body, file).await?;
    if copied != remaining {
        bail!("body ended early");
    }
    Ok(())
}

async fn write_chunked_body(
    stream: &mut TcpStream,
    mut buffered: Vec<u8>,
    file: &mut fs::File,
) -> Result<()> {
    loop {
        let line = take_line(stream, &mut buffered).await?;
        let length = usize::from_str_radix(line.split(';').next().unwrap_or_default(), 16)
            .context("invalid chunk length")?;
        if length == 0 {
            while !take_line(stream, &mut buffered).await?.is_empty() {}
            return Ok(());
        }
        let mut remaining = length;
        if !buffered.is_empty() {
            let take = remaining.min(buffered.len());
            file.write_all(&buffered[..take]).await?;
            buffered.drain(..take);
            remaining -= take;
        }
        let mut chunk = [0u8; 64 * 1024];
        while remaining > 0 {
            let read_len = remaining.min(chunk.len());
            let read = stream.read(&mut chunk[..read_len]).await?;
            if read == 0 {
                bail!("chunked body ended early");
            }
            file.write_all(&chunk[..read]).await?;
            remaining -= read;
        }
        fill(stream, &mut buffered, 2).await?;
        if &buffered[..2] != b"\r\n" {
            bail!("invalid chunk terminator");
        }
        buffered.drain(..2);
    }
}

async fn take_line(stream: &mut TcpStream, buffered: &mut Vec<u8>) -> Result<String> {
    loop {
        if let Some(position) = buffered.windows(2).position(|bytes| bytes == b"\r\n") {
            let line = std::str::from_utf8(&buffered[..position])
                .context("chunk header is not UTF-8")?
                .to_string();
            buffered.drain(..position + 2);
            return Ok(line);
        }
        if buffered.len() > 8192 {
            bail!("chunk header exceeds 8 KiB");
        }
        let mut chunk = [0u8; 4096];
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            bail!("chunked body ended early");
        }
        buffered.extend_from_slice(&chunk[..read]);
    }
}

async fn fill(stream: &mut TcpStream, buffered: &mut Vec<u8>, length: usize) -> Result<()> {
    while buffered.len() < length {
        let mut chunk = [0u8; 64 * 1024];
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            bail!("chunked body ended early");
        }
        buffered.extend_from_slice(&chunk[..read]);
    }
    Ok(())
}

fn dav_relative_path(path: &str) -> Result<PathBuf> {
    if path.starts_with('/') || path.contains('?') {
        bail!("invalid DAV path");
    }
    let mut output = PathBuf::new();
    for segment in path
        .trim_end_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
    {
        let segment = percent_decode_str(segment)
            .decode_utf8()
            .context("invalid DAV path encoding")?
            .into_owned();
        let mut components = Path::new(&segment).components();
        if segment.is_empty()
            || matches!(segment.as_str(), "." | "..")
            || segment.contains(['/', '\\', '\0'])
            || !matches!(components.next(), Some(Component::Normal(_)))
            || components.next().is_some()
        {
            bail!("invalid DAV path");
        }
        output.push(segment);
    }
    Ok(output)
}

async fn existing_target(root: &Path, relative: &Path) -> Result<PathBuf> {
    let target = fs::canonicalize(root.join(relative))
        .await
        .context("read DAV target")?;
    if target.starts_with(root) {
        Ok(target)
    } else {
        bail!("DAV target escapes root")
    }
}

async fn writable_target(root: &Path, relative: &Path) -> Result<PathBuf> {
    let target = root.join(relative);
    let parent = target.parent().context("DAV target has no parent")?;
    let parent = fs::canonicalize(parent).await.context("read DAV parent")?;
    if !parent.starts_with(root) {
        bail!("DAV target escapes root");
    }
    Ok(parent.join(target.file_name().context("DAV target has no name")?))
}

async fn existing_or_writable_target(root: &Path, relative: &Path) -> Result<PathBuf> {
    match existing_target(root, relative).await {
        Ok(target) => Ok(target),
        Err(_) => writable_target(root, relative).await,
    }
}

fn destination_relative(value: &str, token: Option<&str>) -> Result<PathBuf> {
    let path = url::Url::parse(value)
        .ok()
        .map(|url| url.path().to_string())
        .unwrap_or_else(|| value.to_string());
    let path = web_token_path(token, &path).context("Destination lacks DAV token")?;
    dav_relative_path(path)
}

fn lock_permits(locks: &Arc<Mutex<LockState>>, target: &Path, request: &WebRequest) -> bool {
    let mut state = locks.lock().expect("DAV lock mutex poisoned");
    prune_locks(&mut state);
    state
        .locks
        .iter()
        .filter(|(path, _)| target.starts_with(path) || path.starts_with(target))
        .all(|(_, lock)| request_presents_lock(lock, request))
}

fn request_presents_lock(lock: &Lock, request: &WebRequest) -> bool {
    request
        .header("if")
        .is_some_and(|value| value.contains(&lock.token))
        || request
            .header("lock-token")
            .is_some_and(|value| strip_lock_token(value) == lock.token)
}

fn prune_locks(state: &mut LockState) {
    let now = std::time::Instant::now();
    state.locks.retain(|_, lock| lock.expires > now);
}

fn move_locks(locks: &Arc<Mutex<LockState>>, source: &Path, destination: &Path) {
    let mut state = locks.lock().expect("DAV lock mutex poisoned");
    prune_locks(&mut state);
    let moved = state
        .locks
        .extract_if(|path, _| path.starts_with(source))
        .map(|(path, lock)| {
            let suffix = path
                .strip_prefix(source)
                .expect("lock path starts with source");
            (destination.join(suffix), lock)
        })
        .collect::<Vec<_>>();
    state.locks.extend(moved);
}

fn strip_lock_token(value: &str) -> String {
    value.trim().trim_matches(['<', '>']).to_string()
}

async fn prop_response(root: &Path, target: &Path, token: Option<&str>) -> Result<String> {
    let meta = fs::metadata(target).await?;
    let mut href = dav_href(root, target, token)?;
    let name = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    let modified = meta
        .modified()
        .map(OffsetDateTime::from)
        .unwrap_or(OffsetDateTime::UNIX_EPOCH);
    let creation_date = modified
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string());
    let last_modified = modified
        .format(HTTP_DATE_FORMAT)
        .unwrap_or_else(|_| "Thu, 01 Jan 1970 00:00:00 GMT".to_string());
    let resource_type = if meta.is_dir() {
        if !href.ends_with('/') {
            href.push('/');
        }
        "<d:resourcetype><d:collection/></d:resourcetype>"
    } else {
        "<d:resourcetype/>"
    };
    let length = if meta.is_dir() {
        String::new()
    } else {
        format!("<d:getcontentlength>{}</d:getcontentlength>", meta.len())
    };
    Ok(format!(
        "<d:response><d:href>{href}</d:href><d:propstat><d:prop><d:displayname>{}</d:displayname>{resource_type}{length}<d:creationdate>{creation_date}</d:creationdate><d:getlastmodified>{last_modified}</d:getlastmodified><d:getetag>W/\"{}-{}\"</d:getetag><d:supportedlock><d:lockentry><d:lockscope><d:exclusive/></d:lockscope><d:locktype><d:write/></d:locktype></d:lockentry></d:supportedlock></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>",
        xml_escape(name),
        meta.len(),
        modified.unix_timestamp()
    ))
}

fn dav_href(root: &Path, target: &Path, token: Option<&str>) -> Result<String> {
    let relative = target
        .strip_prefix(root)
        .context("DAV target escapes root")?;
    Ok(dav_relative_href(relative, token))
}

fn dav_relative_href(relative: &Path, token: Option<&str>) -> String {
    let mut href = web_root_path(token);
    let segments = relative
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => value.to_str(),
            _ => None,
        });
    let encoded = segments
        .map(|value| utf8_percent_encode(value, NON_ALPHANUMERIC).to_string())
        .collect::<Vec<_>>()
        .join("/");
    href.push_str(&encoded);
    href
}

fn xml_escape(value: &str) -> String {
    html_escape(value).replace('\'', "&apos;")
}

fn copy_path(source: &Path, destination: &Path) -> Result<()> {
    if source.is_dir() {
        std::fs::create_dir(destination)?;
        for entry in std::fs::read_dir(source)? {
            let entry = entry?;
            copy_path(&entry.path(), &destination.join(entry.file_name()))?;
        }
    } else {
        std::fs::copy(source, destination)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web::test_support::{raw_request, request, response_body, response_header};
    use std::{
        net::{Ipv4Addr, SocketAddr},
        sync::Arc,
    };
    use tokio::net::TcpListener;

    async fn test_server(
        root: PathBuf,
        read_only: bool,
        count: usize,
    ) -> (SocketAddr, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let address = listener.local_addr().unwrap();
        let locks = Arc::new(Mutex::new(LockState::default()));
        let task = tokio::spawn(async move {
            for _ in 0..count {
                let (stream, _) = listener.accept().await.unwrap();
                let root = root.clone();
                let locks = Arc::clone(&locks);
                tokio::spawn(async move {
                    serve_dav_connection(stream, root, None, read_only, locks)
                        .await
                        .unwrap();
                });
            }
        });
        (address, task)
    }

    #[tokio::test]
    async fn desktop_request_sequence_supports_chunked_put_copy_move_and_locks() {
        let temp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(temp.path()).unwrap();
        std::fs::write(root.join("existing.txt"), b"existing").unwrap();
        let (address, server) = test_server(root.clone(), false, 12).await;

        assert!(request(address, "/").await.starts_with(b"HTTP/1.1 200 OK"));
        let propfind = crate::web::test_support::request_with_headers(
            address,
            "PROPFIND",
            "/",
            "Depth: 1\r\n",
        )
        .await;
        assert!(propfind.starts_with(b"HTTP/1.1 207 Multi-Status"));
        let body = std::str::from_utf8(response_body(&propfind)).unwrap();
        assert!(body.contains("getlastmodified"));
        assert!(body.contains("GMT"));

        let chunked = raw_request(
            address,
            b"PUT /chunked.txt HTTP/1.1\r\nHost: test\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n",
        )
        .await;
        assert!(chunked.starts_with(b"HTTP/1.1 201 Created"));
        assert_eq!(std::fs::read(root.join("chunked.txt")).unwrap(), b"hello");
        assert_eq!(
            response_body(&request(address, "/chunked.txt").await),
            b"hello"
        );

        let mkcol = raw_request(
            address,
            b"MKCOL /folder HTTP/1.1\r\nHost: test\r\nContent-Length: 0\r\n\r\n",
        )
        .await;
        assert!(mkcol.starts_with(b"HTTP/1.1 201 Created"));
        let copy = raw_request(
            address,
            b"COPY /chunked.txt HTTP/1.1\r\nHost: test\r\nDestination: http://test/copied.txt\r\n\r\n",
        )
        .await;
        assert!(copy.starts_with(b"HTTP/1.1 201 Created"));
        let moved = raw_request(
            address,
            b"MOVE /copied.txt HTTP/1.1\r\nHost: test\r\nDestination: /moved.txt\r\n\r\n",
        )
        .await;
        assert!(moved.starts_with(b"HTTP/1.1 201 Created"));

        let lock = raw_request(
            address,
            b"LOCK /moved.txt HTTP/1.1\r\nHost: test\r\nTimeout: Second-3600\r\nContent-Length: 0\r\n\r\n",
        )
        .await;
        assert!(lock.starts_with(b"HTTP/1.1 200 OK"));
        let lock_token = response_header(&lock, "Lock-Token").unwrap().to_string();
        let blocked = raw_request(
            address,
            b"PUT /moved.txt HTTP/1.1\r\nHost: test\r\nContent-Length: 3\r\n\r\nnew",
        )
        .await;
        assert!(blocked.starts_with(b"HTTP/1.1 423 Locked"));
        let unlock =
            format!("UNLOCK /moved.txt HTTP/1.1\r\nHost: test\r\nLock-Token: {lock_token}\r\n\r\n");
        assert!(
            raw_request(address, unlock.as_bytes())
                .await
                .starts_with(b"HTTP/1.1 204 No Content")
        );
        let overwritten = raw_request(
            address,
            b"PUT /moved.txt HTTP/1.1\r\nHost: test\r\nContent-Length: 3\r\n\r\nnew",
        )
        .await;
        assert!(overwritten.starts_with(b"HTTP/1.1 201 Created"));
        assert!(
            request(address, "/../secret")
                .await
                .starts_with(b"HTTP/1.1 404 Not Found")
        );
        server.await.unwrap();
    }

    #[tokio::test]
    async fn read_only_dav_rejects_writes() {
        let temp = tempfile::tempdir().unwrap();
        let (address, server) =
            test_server(std::fs::canonicalize(temp.path()).unwrap(), true, 1).await;
        let response = raw_request(
            address,
            b"PUT /blocked.txt HTTP/1.1\r\nHost: test\r\nContent-Length: 1\r\n\r\nx",
        )
        .await;
        assert!(response.starts_with(b"HTTP/1.1 403 Forbidden"));
        server.await.unwrap();
    }
}
