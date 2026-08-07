use crate::web::http::{write_web_error, write_web_response, write_web_response_with_headers};
use anyhow::{Context, Result, bail};
use percent_encoding::percent_decode_str;
use rand::RngExt;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
use tempfile::NamedTempFile;
use tokio::{
    fs,
    io::{self, AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::Mutex,
};

const SESSION_TTL: Duration = Duration::from_secs(60 * 60);
const SESSION_ID_BYTES: usize = 16;

#[derive(Debug, Clone)]
pub(crate) struct UploadSession {
    pub(crate) name: String,
    pub(crate) total: u64,
    pub(crate) offset: u64,
    pub(crate) temp_path: PathBuf,
    pub(crate) last_activity: Instant,
}

pub(crate) type UploadSessions = Arc<Mutex<HashMap<String, UploadSession>>>;

pub(crate) fn sessions() -> UploadSessions {
    Arc::new(Mutex::new(HashMap::new()))
}

pub(crate) fn name(target: &str) -> Result<String> {
    let name = query_value(target, "name")?.context("upload name is missing")?;
    let name = percent_decode_str(&name)
        .decode_utf8()
        .context("upload name is not UTF-8")?
        .into_owned();
    if name.is_empty()
        || matches!(name.as_str(), "." | "..")
        || name.contains(['/', '\\'])
        || name.contains('\0')
    {
        bail!("upload name is invalid");
    }
    Ok(name)
}

fn query_value<'a>(target: &'a str, key: &str) -> Result<Option<&'a str>> {
    let (_, query) = target.split_once('?').context("upload query is missing")?;
    Ok(query.split('&').find_map(|part| {
        let (name, value) = part.split_once('=')?;
        (name == key).then_some(value)
    }))
}

fn session_id() -> String {
    let mut bytes = [0u8; SESSION_ID_BYTES];
    rand::rng().fill(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn parse_size(target: &str) -> Result<u64> {
    let value = query_value(target, "size")?.context("upload size is missing")?;
    value.parse().context("upload size is invalid")
}

fn parse_session_id(target: &str) -> Result<String> {
    let value = query_value(target, "upload")?.context("upload session is missing")?;
    if value.is_empty()
        || value.len() > SESSION_ID_BYTES * 2
        || !value.bytes().all(|b| b.is_ascii_hexdigit())
    {
        bail!("upload session is invalid");
    }
    Ok(value.to_string())
}

fn parse_content_range(value: &str) -> Result<(u64, u64, u64)> {
    let value = value
        .strip_prefix("bytes ")
        .context("Content-Range is invalid")?;
    let (range, total) = value.split_once('/').context("Content-Range is invalid")?;
    let (start, end) = range.split_once('-').context("Content-Range is invalid")?;
    let start = start.parse().context("Content-Range start is invalid")?;
    let end = end.parse().context("Content-Range end is invalid")?;
    let total = total.parse().context("Content-Range total is invalid")?;
    if end < start {
        bail!("Content-Range is invalid");
    }
    Ok((start, end, total))
}

pub(crate) async fn cleanup_expired(sessions: &UploadSessions) {
    let expired = {
        let mut guard = sessions.lock().await;
        let now = Instant::now();
        let ids = guard
            .iter()
            .filter(|(_, session)| now.duration_since(session.last_activity) >= SESSION_TTL)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        ids.iter()
            .filter_map(|id| guard.remove(id))
            .collect::<Vec<_>>()
    };
    for session in expired {
        let _ = fs::remove_file(session.temp_path).await;
    }
}

pub(crate) async fn create_session(
    stream: &mut TcpStream,
    upload_dir: &Path,
    target: &str,
    sessions: &UploadSessions,
) -> Result<()> {
    cleanup_expired(sessions).await;
    let name = match name(target) {
        Ok(name) => name,
        Err(err) => {
            return write_web_error(
                stream,
                "400 Bad Request",
                &format!("invalid upload name: {err}"),
            )
            .await;
        }
    };
    let total = match parse_size(target) {
        Ok(total) => total,
        Err(err) => {
            return write_web_error(
                stream,
                "400 Bad Request",
                &format!("invalid upload size: {err}"),
            )
            .await;
        }
    };
    if let Err(err) = fs::create_dir_all(upload_dir).await {
        return write_web_error(
            stream,
            "500 Internal Server Error",
            &format!("create upload directory {}: {err}", upload_dir.display()),
        )
        .await;
    }
    let temp = match NamedTempFile::new_in(upload_dir) {
        Ok(temp) => temp,
        Err(err) => {
            return write_web_error(
                stream,
                "500 Internal Server Error",
                &format!("create upload file: {err}"),
            )
            .await;
        }
    };
    let temp_path = match temp.into_temp_path().keep() {
        Ok(path) => path,
        Err(err) => {
            return write_web_error(
                stream,
                "500 Internal Server Error",
                &format!("keep upload temp file: {err}"),
            )
            .await;
        }
    };
    let id = session_id();
    sessions.lock().await.insert(
        id.clone(),
        UploadSession {
            name,
            total,
            offset: 0,
            temp_path,
            last_activity: Instant::now(),
        },
    );
    write_web_response_with_headers(
        stream,
        "201 Created",
        "text/plain; charset=utf-8",
        &format!("Upload-Id: {id}\r\nUpload-Offset: 0\r\n"),
        id.as_bytes(),
    )
    .await
}

pub(crate) async fn session_head(
    stream: &mut TcpStream,
    target: &str,
    sessions: &UploadSessions,
) -> Result<()> {
    cleanup_expired(sessions).await;
    let id = match parse_session_id(target) {
        Ok(id) => id,
        Err(err) => {
            return write_web_error(
                stream,
                "400 Bad Request",
                &format!("invalid upload session: {err}"),
            )
            .await;
        }
    };
    let session = sessions.lock().await.get(&id).cloned();
    let Some(session) = session else {
        return write_web_error(stream, "404 Not Found", "upload session not found").await;
    };
    write_web_response_with_headers(
        stream,
        "200 OK",
        "text/plain; charset=utf-8",
        &format!(
            "Upload-Offset: {}\r\nUpload-Length: {}\r\n",
            session.offset, session.total
        ),
        b"",
    )
    .await
}

pub(crate) async fn write_upload_chunk(
    stream: &mut TcpStream,
    upload_dir: &Path,
    target: &str,
    content_length: Option<u64>,
    content_range: Option<&str>,
    initial_body: &[u8],
    sessions: &UploadSessions,
) -> Result<()> {
    cleanup_expired(sessions).await;
    let id = match parse_session_id(target) {
        Ok(id) => id,
        Err(err) => {
            return write_web_error(
                stream,
                "400 Bad Request",
                &format!("invalid upload session: {err}"),
            )
            .await;
        }
    };
    let Some(content_length) = content_length else {
        return write_web_error(stream, "411 Length Required", "Content-Length is required").await;
    };
    let Some(content_range) = content_range else {
        return write_web_error(stream, "400 Bad Request", "Content-Range is required").await;
    };
    let (start, end, total) = match parse_content_range(content_range) {
        Ok(range) => range,
        Err(err) => return write_web_error(stream, "400 Bad Request", &err.to_string()).await,
    };
    let expected_length = match end
        .checked_sub(start)
        .and_then(|length| length.checked_add(1))
    {
        Some(length) if total > 0 && end < total => length,
        _ => {
            return write_web_error(stream, "400 Bad Request", "Content-Range is invalid").await;
        }
    };
    if content_length != expected_length
        || u64::try_from(initial_body.len()).unwrap_or(u64::MAX) > content_length
    {
        return write_web_error(
            stream,
            "400 Bad Request",
            "upload body length does not match Content-Range",
        )
        .await;
    }
    let Some(mut session) = sessions.lock().await.remove(&id) else {
        return write_web_error(stream, "404 Not Found", "upload session not found").await;
    };
    if session.total != total || session.offset != start {
        let offset = session.offset;
        sessions.lock().await.insert(id, session);
        return write_web_error(
            stream,
            "409 Conflict",
            &format!("upload offset is {offset}"),
        )
        .await;
    }
    let result = async {
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(&session.temp_path)
            .await
            .context("open upload temp file")?;
        file.write_all(initial_body)
            .await
            .context("write upload body")?;
        let remaining = content_length - u64::try_from(initial_body.len()).unwrap_or(0);
        let mut body = stream.take(remaining);
        let copied = io::copy(&mut body, &mut file)
            .await
            .context("write upload body")?;
        if copied != remaining {
            return Ok::<bool, anyhow::Error>(false);
        }
        file.flush().await.context("flush upload file")?;
        Ok(true)
    }
    .await;
    match result {
        Ok(true) => {}
        Ok(false) => {
            let _ = fs::remove_file(&session.temp_path).await;
            return write_web_error(stream, "400 Bad Request", "upload body ended early").await;
        }
        Err(err) => {
            let _ = fs::remove_file(&session.temp_path).await;
            return write_web_error(
                stream,
                "500 Internal Server Error",
                &format!("write upload file: {err}"),
            )
            .await;
        }
    }
    session.offset = end
        .checked_add(1)
        .context("Content-Range end overflows upload offset")?;
    session.last_activity = Instant::now();
    if session.offset < session.total {
        let offset = session.offset;
        sessions.lock().await.insert(id, session);
        return write_web_response_with_headers(
            stream,
            "204 No Content",
            "text/plain; charset=utf-8",
            &format!("Upload-Offset: {offset}\r\n"),
            b"",
        )
        .await;
    }
    let target_path = upload_dir.join(&session.name);
    if let Err(err) = replace_file(&session.temp_path, &target_path).await {
        let _ = fs::remove_file(&session.temp_path).await;
        return write_web_error(
            stream,
            "500 Internal Server Error",
            &format!("replace upload file: {err}"),
        )
        .await;
    }
    println!("ii web: uploaded {}", target_path.display());
    write_web_response_with_headers(
        stream,
        "201 Created",
        "text/plain; charset=utf-8",
        &format!("Upload-Offset: {}\r\n", session.offset),
        format!("saved: {}", session.name).as_bytes(),
    )
    .await
}

pub(crate) async fn cleanup_all(sessions: &UploadSessions) {
    let active = sessions
        .lock()
        .await
        .drain()
        .map(|(_, session)| session)
        .collect::<Vec<_>>();
    for session in active {
        let _ = fs::remove_file(session.temp_path).await;
    }
}

async fn replace_file(temp_path: &Path, target_path: &Path) -> std::io::Result<()> {
    #[cfg(not(windows))]
    {
        fs::rename(temp_path, target_path).await
    }

    #[cfg(windows)]
    {
        replace_file_windows(temp_path, target_path)
    }
}

#[cfg(windows)]
fn replace_file_windows(temp_path: &Path, target_path: &Path) -> std::io::Result<()> {
    use std::{ffi::c_void, os::windows::ffi::OsStrExt, ptr};

    unsafe extern "system" {
        fn ReplaceFileW(
            replaced_file_name: *const u16,
            replacement_file_name: *const u16,
            backup_file_name: *const u16,
            replace_flags: u32,
            exclude: *mut c_void,
            reserved: *mut c_void,
        ) -> i32;
        fn MoveFileExW(
            existing_file_name: *const u16,
            new_file_name: *const u16,
            flags: u32,
        ) -> i32;
    }

    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;

    let temp_path = temp_path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let target_path = target_path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();

    // ReplaceFileW keeps an existing target visible until replacement succeeds.
    if unsafe {
        ReplaceFileW(
            target_path.as_ptr(),
            temp_path.as_ptr(),
            ptr::null(),
            0,
            ptr::null_mut(),
            ptr::null_mut(),
        )
    } != 0
    {
        return Ok(());
    }

    let replace_error = std::io::Error::last_os_error();
    if replace_error.kind() != std::io::ErrorKind::NotFound {
        return Err(replace_error);
    }
    if unsafe {
        MoveFileExW(
            temp_path.as_ptr(),
            target_path.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } != 0
    {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

pub(crate) fn html_controls() -> &'static str {
    r#"<section class="upload"><input id="upload" type="file" multiple aria-label="Upload files"><button id="upload-button" type="button">Upload</button><output id="upload-status" aria-live="polite"></output></section><script>const input=document.getElementById('upload');const button=document.getElementById('upload-button');const status=document.getElementById('upload-status');const keyFor=f=>'ii-upload:'+location.pathname+':'+f.name+':'+f.size+':'+f.lastModified;const uploadUrl=(name,id)=>'upload?name='+encodeURIComponent(name)+(id?'&upload='+encodeURIComponent(id):'');async function resumable(f){if(f.size===0){const r=await fetch(uploadUrl(f.name),{method:'POST',body:f});if(!r.ok)throw new Error(await r.text());return await r.text();}const key=keyFor(f);for(let attempt=0;attempt<2;attempt++){let id=localStorage.getItem(key);if(!id){const r=await fetch('upload/init?name='+encodeURIComponent(f.name)+'&size='+f.size,{method:'POST'});if(!r.ok)throw new Error(await r.text());id=(await r.text()).trim();localStorage.setItem(key,id);}const h=await fetch(uploadUrl(f.name,id),{method:'HEAD'});if(h.status===404){localStorage.removeItem(key);continue;}if(!h.ok)throw new Error(await h.text());let offset=Number(h.headers.get('Upload-Offset')||'0');let restart=false;const step=1024*1024;while(offset<f.size){const end=Math.min(offset+step,f.size)-1;const chunk=f.slice(offset,end+1);const r=await fetch(uploadUrl(f.name,id),{method:'PATCH',headers:{'Content-Range':'bytes '+offset+'-'+end+'/'+f.size},body:chunk});if(r.status===404){localStorage.removeItem(key);restart=true;break;}if(!r.ok)throw new Error(await r.text());offset=Number(r.headers.get('Upload-Offset')||String(end+1));localStorage.setItem(key,id);}if(restart)continue;localStorage.removeItem(key);return 'saved: '+f.name;}throw new Error('upload session expired; retry upload');}button.addEventListener('click',async()=>{const files=[...input.files];if(!files.length)return;button.disabled=true;status.textContent='';for(const file of files){const row=document.createElement('div');row.textContent=file.name;status.append(row);try{row.textContent=await resumable(file);}catch(error){row.textContent=file.name+': '+error;}}button.disabled=false;input.value='';});</script>"#
}

pub(crate) async fn create_file(upload_dir: &Path, name: &str) -> Result<(PathBuf, NamedTempFile)> {
    fs::create_dir_all(upload_dir)
        .await
        .with_context(|| format!("create upload directory {}", upload_dir.display()))?;
    let temp = NamedTempFile::new_in(upload_dir)
        .with_context(|| format!("create upload file in {}", upload_dir.display()))?;
    Ok((upload_dir.join(name), temp))
}

pub(crate) async fn write_upload(
    stream: &mut TcpStream,
    upload_dir: &Path,
    target: &str,
    content_length: Option<u64>,
    initial_body: &[u8],
) -> Result<()> {
    let name = match name(target) {
        Ok(name) => name,
        Err(err) => {
            let message = format!("invalid upload name: {err}");
            return write_web_error(stream, "400 Bad Request", &message).await;
        }
    };
    let Some(content_length) = content_length else {
        return write_web_error(stream, "411 Length Required", "Content-Length is required").await;
    };
    let initial_length = u64::try_from(initial_body.len()).context("upload body is too large")?;
    if initial_length > content_length {
        return write_web_error(
            stream,
            "400 Bad Request",
            "upload body exceeds Content-Length",
        )
        .await;
    }

    let (path, temp) = match create_file(upload_dir, &name).await {
        Ok(file) => file,
        Err(err) => {
            let message = format!("create upload file: {err}");
            return write_web_error(stream, "500 Internal Server Error", &message).await;
        }
    };
    let mut file = match temp.reopen() {
        Ok(file) => fs::File::from_std(file),
        Err(err) => {
            let message = format!("open upload file: {err}");
            return write_web_error(stream, "500 Internal Server Error", &message).await;
        }
    };
    let remaining = content_length - initial_length;
    let write_result = async {
        file.write_all(initial_body)
            .await
            .context("write upload body")?;
        let mut body = stream.take(remaining);
        let copied = io::copy(&mut body, &mut file)
            .await
            .context("write upload body")?;
        if copied != remaining {
            bail!("upload body ended early");
        }
        file.flush().await.context("flush upload file")?;
        Ok(())
    }
    .await;
    if let Err(err) = write_result {
        drop(file);
        let message = format!("upload failed: {err}");
        return write_web_error(stream, "400 Bad Request", &message).await;
    }
    drop(file);
    let temp_path = match temp.into_temp_path().keep() {
        Ok(path) => path,
        Err(err) => {
            let message = format!("keep upload temp file: {err}");
            return write_web_error(stream, "500 Internal Server Error", &message).await;
        }
    };
    if let Err(err) = replace_file(&temp_path, &path).await {
        let _ = fs::remove_file(&temp_path).await;
        let message = format!("replace upload file: {err}");
        return write_web_error(stream, "500 Internal Server Error", &message).await;
    }

    println!("ii web: uploaded {}", path.display());
    let message = format!(
        "saved: {}",
        path.file_name().unwrap_or_default().to_string_lossy()
    );
    write_web_response(
        stream,
        "201 Created",
        "text/plain; charset=utf-8",
        message.as_bytes(),
    )
    .await
}
