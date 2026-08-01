use crate::{
    transport::progress::fmt_bytes,
    web::http::{
        WebFileRange, WebRange, html_escape, web_root_path, write_web_error, write_web_redirect,
        write_web_response_for_method,
    },
};
use anyhow::{Context, Result, bail};
use percent_encoding::{NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};
use std::{
    ffi::OsStr,
    io::SeekFrom,
    path::{Component, Path, PathBuf},
};
use time::{OffsetDateTime, format_description::FormatItem, macros::format_description};
use tokio::{
    fs,
    io::{self, AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
    net::TcpStream,
};

const WEB_DIRECTORY_TIME_FORMAT: &[FormatItem<'static>] =
    format_description!("[year]-[month]-[day] [hour]:[minute]");

struct WebDirectoryEntry {
    name: String,
    is_dir: bool,
    modified: String,
    size: String,
}
pub(crate) async fn directory_root(
    start_dir: &Path,
    requested_dir: Option<&Path>,
) -> Result<PathBuf> {
    let path = match requested_dir {
        Some(path) if path.is_absolute() => path.to_path_buf(),
        Some(path) => start_dir.join(path),
        None => start_dir.to_path_buf(),
    };
    let root = fs::canonicalize(&path)
        .await
        .with_context(|| format!("read web directory {}", path.display()))?;
    let metadata = fs::metadata(&root)
        .await
        .with_context(|| format!("read web directory {}", root.display()))?;
    if !metadata.is_dir() {
        bail!("web path is not a directory: {}", path.display());
    }
    Ok(root)
}
pub(crate) async fn write_directory(
    stream: &mut TcpStream,
    root: &Path,
    web_token: Option<&str>,
    path: &str,
    request_target: &str,
    range: &WebRange,
    head: bool,
) -> Result<()> {
    let Some((target, segments, trailing_slash)) = web_directory_target(root, path).await else {
        return write_web_error(stream, "404 Not Found", "not found").await;
    };
    let metadata = match fs::metadata(&target).await {
        Ok(metadata) => metadata,
        Err(_) => return write_web_error(stream, "404 Not Found", "not found").await,
    };

    if metadata.is_dir() {
        if !path.is_empty() && !trailing_slash {
            return write_web_redirect(stream, &format!("{request_target}/")).await;
        }
        let body = match web_directory_page_body(root, &target, web_token, &segments).await {
            Ok(body) => body,
            Err(_) => return write_web_error(stream, "404 Not Found", "not found").await,
        };
        return write_web_response_for_method(
            stream,
            "200 OK",
            "text/html; charset=utf-8",
            "",
            body.as_bytes(),
            head,
        )
        .await;
    }

    if trailing_slash {
        return write_web_error(stream, "404 Not Found", "not found").await;
    }
    let file = match fs::File::open(&target).await {
        Ok(file) => file,
        Err(_) => return write_web_error(stream, "404 Not Found", "not found").await,
    };
    write_web_file(stream, file, &target, metadata.len(), range, head).await
}

async fn web_directory_target(root: &Path, path: &str) -> Option<(PathBuf, Vec<String>, bool)> {
    if path.starts_with('/') || path.contains('?') {
        return None;
    }
    let trailing_slash = path.ends_with('/');
    let path = if trailing_slash {
        &path[..path.len().checked_sub(1)?]
    } else {
        path
    };
    let mut segments = Vec::new();
    if !path.is_empty() {
        for encoded in path.split('/') {
            if encoded.is_empty() {
                return None;
            }
            let segment = percent_decode_str(encoded).decode_utf8().ok()?.into_owned();
            let mut components = Path::new(&segment).components();
            if segment.is_empty()
                || matches!(segment.as_str(), "." | "..")
                || segment.contains(['/', '\\', '\0'])
                || !matches!(components.next(), Some(Component::Normal(_)))
                || components.next().is_some()
            {
                return None;
            }
            segments.push(segment);
        }
    }

    let target = segments
        .iter()
        .fold(root.to_path_buf(), |path, segment| path.join(segment));
    let target = fs::canonicalize(target).await.ok()?;
    target
        .starts_with(root)
        .then_some((target, segments, trailing_slash))
}

async fn web_directory_page_body(
    root: &Path,
    directory: &Path,
    web_token: Option<&str>,
    segments: &[String],
) -> Result<String> {
    let mut entries = fs::read_dir(directory)
        .await
        .with_context(|| format!("read web directory {}", directory.display()))?;
    let mut listed = Vec::new();
    while let Some(entry) = entries
        .next_entry()
        .await
        .context("read web directory entry")?
    {
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str().map(str::to_owned) else {
            continue;
        };
        let target = match fs::canonicalize(entry.path()).await {
            Ok(target) if target.starts_with(root) => target,
            _ => continue,
        };
        let metadata = match fs::metadata(target).await {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        listed.push(WebDirectoryEntry {
            name,
            is_dir: metadata.is_dir(),
            modified: web_directory_modified(&metadata),
            size: if metadata.is_dir() {
                "-".to_string()
            } else {
                fmt_bytes(metadata.len())
            },
        });
    }
    listed.sort_by(|left, right| left.name.cmp(&right.name));

    let mut rows = String::new();
    if !segments.is_empty() {
        let parent = web_directory_href(&segments[..segments.len() - 1], true);
        rows.push_str(&format!("<a href=\"{parent}\">../</a>\n"));
    }
    for entry in listed {
        let mut entry_segments = segments.to_vec();
        entry_segments.push(entry.name.clone());
        let href = web_directory_href(&entry_segments, entry.is_dir);
        let label = if entry.is_dir {
            format!("{}/", entry.name)
        } else {
            entry.name
        };
        rows.push_str(&format!(
            "<a href=\"{href}\">{}</a>  {}  {}\n",
            html_escape(&label),
            entry.modified,
            entry.size,
        ));
    }

    let display_path = if segments.is_empty() {
        "/".to_string()
    } else {
        format!("/{}/", segments.join("/"))
    };
    let title = format!("Index of {display_path}");
    let mut body = String::from(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><style>body{box-sizing:border-box;max-width:58rem;margin:0 auto;padding:1.5rem 1rem 2rem;background:#fff;color:#111;font-family:ui-monospace,SFMono-Regular,Consolas,monospace;font-size:14px;line-height:1.45}h1{margin:0 0 1rem;font-size:1.25rem;font-weight:600;overflow-wrap:anywhere}.upload{display:grid;gap:.625rem;margin:0 0 1.25rem}.upload input,.upload button{box-sizing:border-box;width:100%;min-height:2.75rem;padding:.625rem;border:1px solid #777;border-radius:0;background:#fff;color:#111;font:inherit}.upload button{background:#1769aa;border-color:#1769aa;color:#fff;font-weight:600}.upload button:disabled{opacity:.6}output{display:grid;gap:.375rem;overflow-wrap:anywhere;color:#555}pre{margin:1rem 0;overflow-x:auto;font:inherit}a{color:#0645ad;text-decoration:none}a:hover{text-decoration:underline}@media (max-width:30rem){body{padding:1rem .75rem 1.5rem;font-size:13px}h1{font-size:1.125rem}}</style><base href=\"",
    );
    body.push_str(&html_escape(&web_root_path(web_token)));
    body.push_str("\"><title>");
    body.push_str(&html_escape(&title));
    body.push_str("</title></head><body><h1>");
    body.push_str(&html_escape(&title));
    body.push_str("</h1>");
    body.push_str(web_upload_controls());
    body.push_str("<hr><pre>Name                             Last modified       Size\n----------------------------------------------------------------\n");
    body.push_str(&rows);
    body.push_str("</pre><hr></body></html>");
    Ok(body)
}

fn web_directory_modified(metadata: &std::fs::Metadata) -> String {
    metadata
        .modified()
        .ok()
        .and_then(|time| {
            OffsetDateTime::from(time)
                .format(WEB_DIRECTORY_TIME_FORMAT)
                .ok()
        })
        .unwrap_or_else(|| "-".to_string())
}

pub(crate) fn web_directory_href(segments: &[String], is_dir: bool) -> String {
    if segments.is_empty() {
        return "./".to_string();
    }
    let mut href = segments
        .iter()
        .map(|segment| utf8_percent_encode(segment, NON_ALPHANUMERIC).to_string())
        .collect::<Vec<_>>()
        .join("/");
    if is_dir {
        href.push('/');
    }
    href
}

fn web_upload_controls() -> &'static str {
    "<section class=\"upload\"><input id=\"upload\" type=\"file\" multiple aria-label=\"Upload files\"><button id=\"upload-button\" type=\"button\">Upload</button><output id=\"upload-status\" aria-live=\"polite\"></output></section><script>const input=document.getElementById('upload');const button=document.getElementById('upload-button');const status=document.getElementById('upload-status');button.addEventListener('click',async()=>{const files=[...input.files];if(!files.length)return;button.disabled=true;status.textContent='';for(const file of files){const row=document.createElement('div');row.textContent=file.name;status.append(row);try{const response=await fetch('upload?name='+encodeURIComponent(file.name),{method:'POST',body:file});const text=await response.text();row.textContent=response.ok?text:file.name+': '+text;}catch(error){row.textContent=file.name+': '+error;}}button.disabled=false;input.value='';});</script>"
}

async fn write_web_file(
    stream: &mut TcpStream,
    mut file: fs::File,
    path: &Path,
    size: u64,
    range: &WebRange,
    head: bool,
) -> Result<()> {
    let (status, start, length, range_header) = match web_file_range(range, size) {
        WebFileRange::Full => ("200 OK", 0, size, String::new()),
        WebFileRange::Partial { start, end } => (
            "206 Partial Content",
            start,
            end - start + 1,
            format!("Content-Range: bytes {start}-{end}/{size}\r\n"),
        ),
        WebFileRange::Unsatisfiable => {
            return write_web_response_for_method(
                stream,
                "416 Range Not Satisfiable",
                "text/plain; charset=utf-8",
                &format!("Accept-Ranges: bytes\r\nContent-Range: bytes */{size}\r\n"),
                b"",
                head,
            )
            .await;
        }
    };
    file.seek(SeekFrom::Start(start))
        .await
        .context("seek web file")?;
    let headers = format!("Accept-Ranges: bytes\r\n{range_header}");
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {}\r\n{headers}Content-Length: {length}\r\nConnection: close\r\n\r\n",
        web_file_content_type(path),
    );
    stream
        .write_all(header.as_bytes())
        .await
        .context("write web file headers")?;
    if !head {
        let mut file = file.take(length);
        io::copy(&mut file, stream)
            .await
            .context("write web file")?;
    }
    stream.shutdown().await.context("finish web file")?;
    Ok(())
}

fn web_file_range(range: &WebRange, size: u64) -> WebFileRange {
    let WebRange::Header(value) = range else {
        return if matches!(range, WebRange::None) {
            WebFileRange::Full
        } else {
            WebFileRange::Unsatisfiable
        };
    };
    let Some(spec) = value.strip_prefix("bytes=") else {
        return WebFileRange::Unsatisfiable;
    };
    if size == 0 || spec.contains(',') {
        return WebFileRange::Unsatisfiable;
    }
    let Some((start, end)) = spec.split_once('-') else {
        return WebFileRange::Unsatisfiable;
    };
    if start.is_empty() {
        let Ok(suffix) = end.parse::<u64>() else {
            return WebFileRange::Unsatisfiable;
        };
        if suffix == 0 {
            return WebFileRange::Unsatisfiable;
        }
        let start = size.saturating_sub(suffix);
        return WebFileRange::Partial {
            start,
            end: size - 1,
        };
    }
    let Ok(start) = start.parse::<u64>() else {
        return WebFileRange::Unsatisfiable;
    };
    if start >= size {
        return WebFileRange::Unsatisfiable;
    }
    let end = if end.is_empty() {
        size - 1
    } else {
        let Ok(end) = end.parse::<u64>() else {
            return WebFileRange::Unsatisfiable;
        };
        if start > end {
            return WebFileRange::Unsatisfiable;
        }
        end.min(size - 1)
    };
    WebFileRange::Partial { start, end }
}

pub(crate) fn web_file_content_type(path: &Path) -> &'static str {
    let extension = path.extension().and_then(OsStr::to_str).unwrap_or_default();
    if extension.eq_ignore_ascii_case("mp4") || extension.eq_ignore_ascii_case("m4v") {
        "video/mp4"
    } else if extension.eq_ignore_ascii_case("mov") {
        "video/quicktime"
    } else if extension.eq_ignore_ascii_case("webm") {
        "video/webm"
    } else if extension.eq_ignore_ascii_case("ogv") {
        "video/ogg"
    } else if extension.eq_ignore_ascii_case("ogg") {
        "audio/ogg"
    } else if extension.eq_ignore_ascii_case("mp3") {
        "audio/mpeg"
    } else if extension.eq_ignore_ascii_case("m4a") {
        "audio/mp4"
    } else if extension.eq_ignore_ascii_case("aac") {
        "audio/aac"
    } else if extension.eq_ignore_ascii_case("wav") {
        "audio/wav"
    } else if extension.eq_ignore_ascii_case("opus") {
        "audio/opus"
    } else if extension.eq_ignore_ascii_case("pdf") {
        "application/pdf"
    } else if extension.eq_ignore_ascii_case("png") {
        "image/png"
    } else if extension.eq_ignore_ascii_case("jpg") || extension.eq_ignore_ascii_case("jpeg") {
        "image/jpeg"
    } else if extension.eq_ignore_ascii_case("gif") {
        "image/gif"
    } else if extension.eq_ignore_ascii_case("webp") {
        "image/webp"
    } else if extension.eq_ignore_ascii_case("svg") {
        "image/svg+xml"
    } else if extension.eq_ignore_ascii_case("avif") {
        "image/avif"
    } else {
        "application/octet-stream"
    }
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn href_encodes_each_path_segment() {
        assert_eq!(web_directory_href(&[], true), "./");
        assert_eq!(
            web_directory_href(&["nested".to_string(), "two words".to_string()], true),
            "nested/two%20words/"
        );
    }

    #[test]
    fn file_content_types_cover_media_documents_and_images() {
        for (path, content_type) in [
            ("movie.MP4", "video/mp4"),
            ("movie.webm", "video/webm"),
            ("song.mp3", "audio/mpeg"),
            ("document.pdf", "application/pdf"),
            ("image.png", "image/png"),
            ("unknown.data", "application/octet-stream"),
        ] {
            assert_eq!(web_file_content_type(Path::new(path)), content_type);
        }
    }
}
