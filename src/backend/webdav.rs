use crate::{
    command::RecvArgs,
    storage,
    ticket::{PayloadKind, Ticket, WebDavPortableCredentials},
    transport::{
        p2p::{FilePlan, RecvTrace},
        progress::TransferProgress,
        source::{Source, unique_object_id},
    },
};
use anyhow::{Context, Result, bail};
use futures_util::TryStreamExt;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tempfile::NamedTempFile;
use tokio::{
    fs,
    io::{self, AsyncWrite, AsyncWriteExt},
};
use tokio_util::io::ReaderStream;

pub(crate) struct WebDavUpload {
    pub(crate) object_key: String,
}

pub(crate) async fn upload(
    source: &Source,
    profile: &storage::WebDavProfile,
    show_progress: bool,
) -> Result<WebDavUpload> {
    let client = storage::build_webdav_client(profile)?;
    let object_key = match source.content_md5() {
        Some(content_md5) => {
            storage::content_addressed_object_key(&profile.remote_dir, content_md5)
        }
        None => {
            storage::normalized_object_key(&profile.remote_dir, &unique_object_id(), source.name())
        }
    };
    ensure_parent_dirs(&client, &object_key).await?;
    if object_exists(&client, &object_key).await? {
        return Ok(WebDavUpload { object_key });
    }

    let file = source.open_file().await?;
    let progress = Arc::new(Mutex::new(TransferProgress::new(
        "ii send",
        show_progress,
        source.size(),
        0,
    )));
    let progress_for_stream = Arc::clone(&progress);
    let stream = ReaderStream::new(file).inspect_ok(move |bytes| {
        if let Ok(mut progress) = progress_for_stream.lock() {
            progress.advance(bytes.len() as u64);
        }
    });
    let body = reqwest::Body::wrap_stream(stream);
    let response = client
        .start_request(reqwest::Method::PUT, &object_key)
        .await
        .with_context(|| format!("prepare WebDAV upload {object_key}"))?
        .header("content-type", "application/octet-stream")
        .header("content-length", source.size().unwrap_or(0).to_string())
        .body(body)
        .send()
        .await
        .with_context(|| format!("upload WebDAV object {object_key}"))?;
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        bail!("WebDAV upload failed with status {status}");
    }
    if let Ok(mut progress) = progress.lock() {
        progress.finish();
    }
    Ok(WebDavUpload { object_key })
}

async fn ensure_parent_dirs(client: &crate::webdav::Client, object_key: &str) -> Result<()> {
    let mut current = String::new();
    let parts = object_key.trim_matches('/').split('/').collect::<Vec<_>>();
    for part in parts.iter().take(parts.len().saturating_sub(1)) {
        if part.is_empty() {
            continue;
        }
        if !current.is_empty() {
            current.push('/');
        }
        current.push_str(part);
        match client.mkcol(&current).await {
            Ok(status) if (200..300).contains(&status) || matches!(status, 405 | 409) => {}
            Ok(status) => bail!("create WebDAV dir {current} failed with status {status}"),
            Err(err) => return Err(err).with_context(|| format!("create WebDAV dir {current}")),
        }
    }
    Ok(())
}

async fn object_exists(client: &crate::webdav::Client, object_key: &str) -> Result<bool> {
    let response = client.propfind(object_key).await?;
    match response.status() {
        status if (200..300).contains(&status) => response
            .is_multistatus()
            .with_context(|| format!("parse WebDAV object response for {object_key}")),
        404 => Ok(false),
        status => bail!("check WebDAV object {object_key} failed with status {status}"),
    }
}

pub(crate) async fn recv_webdav(
    args: RecvArgs,
    ticket: Ticket,
    out_dir: PathBuf,
    file_target: Option<(PathBuf, FilePlan)>,
    mut trace: RecvTrace,
    show_progress: bool,
) -> Result<()> {
    let webdav = ticket
        .webdav_route()
        .context("webdav ticket missing route")?
        .clone();
    trace.info(format_args!("using webdav object {}", webdav.object_key));
    let (profile, save_after_success) = match &webdav.portable {
        Some(portable) => {
            let profile = webdav_profile_from_portable(portable)?;
            let save = portable_webdav_config(&webdav.profile, &profile)?;
            (profile, Some(save))
        }
        None => {
            let selection = storage::load_or_prompt_webdav_profile_named(&webdav.profile)?;
            let save = selection
                .save_after_success
                .then_some((selection.path.clone(), selection.config.clone()));
            (selection.profile, save)
        }
    };
    let client = storage::build_webdav_client(&profile)?;

    let bytes_written = match ticket.kind() {
        PayloadKind::File | PayloadKind::Stdin => {
            if args.stdout {
                download_webdav_to_stdout(
                    &client,
                    &webdav.object_key,
                    ticket.size(),
                    show_progress,
                    &mut trace,
                )
                .await?
            } else {
                let (path, plan) = file_target.expect("file target exists");
                let resume_from = match plan {
                    FilePlan::Download { resume_from } => resume_from,
                    FilePlan::Skip => 0,
                };
                download_webdav_to_file(
                    &client,
                    &webdav.object_key,
                    path,
                    resume_from,
                    ticket.size(),
                    show_progress,
                    &mut trace,
                )
                .await?
            }
        }
        PayloadKind::Dir => {
            if args.stdout {
                bail!("--stdout is not supported for directory tickets");
            }
            download_webdav_tar(
                &client,
                &webdav.object_key,
                out_dir,
                ticket.size(),
                show_progress,
                &mut trace,
            )
            .await?
        }
    };
    if let Some((path, config)) = save_after_success {
        storage::save_config(&path, &config)?;
    }
    trace.step("receive payload");
    trace.info(format_args!("received {} bytes", bytes_written));
    try_delete_webdav(
        &client,
        &webdav.object_key,
        webdav.delete_after_recv,
        &mut trace,
    )
    .await;
    trace.finish(bytes_written);
    Ok(())
}

pub(crate) async fn try_delete_webdav_for_ticket(
    webdav: crate::ticket::WebDavTicket,
    trace: &mut RecvTrace,
) {
    if !webdav.delete_after_recv {
        return;
    }
    let result = async {
        let (profile, save_after_success) = match &webdav.portable {
            Some(portable) => {
                let profile = webdav_profile_from_portable(portable)?;
                let save = portable_webdav_config(&webdav.profile, &profile)?;
                (profile, Some(save))
            }
            None => {
                let selection = storage::load_or_prompt_webdav_profile_named(&webdav.profile)?;
                let save = selection
                    .save_after_success
                    .then_some((selection.path.clone(), selection.config.clone()));
                (selection.profile, save)
            }
        };
        let client = storage::build_webdav_client(&profile)?;
        try_delete_webdav(&client, &webdav.object_key, true, trace).await;
        if let Some((path, config)) = save_after_success {
            storage::save_config(&path, &config)?;
        }
        Ok::<(), anyhow::Error>(())
    }
    .await;
    if let Err(err) = result {
        trace.info(format_args!("webdav delete skipped: {err:#}"));
    }
}

fn portable_webdav_config(
    profile_name: &str,
    profile: &storage::WebDavProfile,
) -> Result<(PathBuf, storage::IiConfig)> {
    let path = storage::default_config_path()?;
    let mut config = storage::load_config(&path)?;
    config
        .storage
        .webdav
        .insert(profile_name.to_string(), profile.clone());
    Ok((path, config))
}

fn webdav_profile_from_portable(
    portable: &WebDavPortableCredentials,
) -> Result<storage::WebDavProfile> {
    let auth = match portable.auth.as_str() {
        "basic" => storage::WebDavAuth::Basic,
        "digest" => storage::WebDavAuth::Digest,
        other => bail!("unsupported WebDAV auth {other}"),
    };
    Ok(storage::WebDavProfile {
        url: portable.url.clone(),
        username: portable.username.clone(),
        password: portable.password.clone(),
        remote_dir: "ii/".to_string(),
        auth,
    })
}

async fn download_webdav_to_file(
    client: &crate::webdav::Client,
    object_key: &str,
    path: PathBuf,
    resume_from: u64,
    total_size: Option<u64>,
    show_progress: bool,
    trace: &mut RecvTrace,
) -> Result<u64> {
    trace.info(format_args!("download webdav file to {}", path.display()));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await.ok();
    }
    let mut append = resume_from > 0;
    let mut response = webdav_get(client, object_key, resume_from).await?;
    if resume_from > 0 && response.status().as_u16() == 200 {
        append = false;
        response = webdav_get(client, object_key, 0).await?;
    }
    ensure_webdav_success(response.status().as_u16())?;
    let mut file = if append {
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await
            .with_context(|| format!("open destination {}", path.display()))?
    } else {
        fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&path)
            .await
            .with_context(|| format!("open destination {}", path.display()))?
    };
    let completed = if append { resume_from } else { 0 };
    let mut progress = TransferProgress::new("ii recv", show_progress, total_size, completed);
    let bytes = copy_webdav_response_with_progress(response, &mut file, &mut progress)
        .await
        .with_context(|| format!("write destination {}", path.display()))?;
    progress.finish();
    file.flush()
        .await
        .with_context(|| format!("flush destination {}", path.display()))?;
    Ok(bytes)
}

async fn download_webdav_to_stdout(
    client: &crate::webdav::Client,
    object_key: &str,
    total_size: Option<u64>,
    show_progress: bool,
    trace: &mut RecvTrace,
) -> Result<u64> {
    trace.info("download webdav file to stdout");
    let response = webdav_get(client, object_key, 0).await?;
    ensure_webdav_success(response.status().as_u16())?;
    let mut stdout = io::stdout();
    let mut progress = TransferProgress::new("ii recv", show_progress, total_size, 0);
    let bytes = copy_webdav_response_with_progress(response, &mut stdout, &mut progress)
        .await
        .context("write stdout")?;
    progress.finish();
    stdout.flush().await.ok();
    Ok(bytes)
}

async fn download_webdav_tar(
    client: &crate::webdav::Client,
    object_key: &str,
    out_dir: PathBuf,
    total_size: Option<u64>,
    show_progress: bool,
    trace: &mut RecvTrace,
) -> Result<u64> {
    trace.info(format_args!("download webdav tar to {}", out_dir.display()));
    fs::create_dir_all(&out_dir)
        .await
        .with_context(|| format!("create output dir {}", out_dir.display()))?;
    let response = webdav_get(client, object_key, 0).await?;
    ensure_webdav_success(response.status().as_u16())?;
    let temp = NamedTempFile::new().context("create temp tar")?;
    let temp_path = temp.path().to_path_buf();
    let mut file = fs::File::from_std(temp.reopen().context("reopen temp tar")?);
    let mut progress = TransferProgress::new("ii recv", show_progress, total_size, 0);
    let bytes = copy_webdav_response_with_progress(response, &mut file, &mut progress)
        .await
        .context("buffer webdav tar")?;
    progress.finish();
    file.flush().await.context("flush temp tar")?;
    let extract_path = out_dir.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let file = std::fs::File::open(&temp_path).context("open tar")?;
        let mut archive = tar::Archive::new(file);
        archive.unpack(&extract_path).context("unpack tar")?;
        Ok(())
    })
    .await
    .context("extract webdav tar task")??;
    Ok(bytes)
}

async fn webdav_get(
    client: &crate::webdav::Client,
    object_key: &str,
    resume_from: u64,
) -> Result<reqwest::Response> {
    let mut request = client
        .start_request(reqwest::Method::GET, object_key)
        .await
        .with_context(|| format!("prepare WebDAV download {object_key}"))?;
    if resume_from > 0 {
        request = request.header("range", format!("bytes={resume_from}-"));
    }
    request
        .send()
        .await
        .with_context(|| format!("download WebDAV object {object_key}"))
}

fn ensure_webdav_success(status: u16) -> Result<()> {
    if (200..300).contains(&status) {
        Ok(())
    } else {
        bail!("WebDAV download failed with status {status}")
    }
}

async fn copy_webdav_response_with_progress<W>(
    mut response: reqwest::Response,
    writer: &mut W,
    progress: &mut TransferProgress,
) -> Result<u64>
where
    W: AsyncWrite + Unpin,
{
    let mut written = 0u64;
    while let Some(chunk) = response.chunk().await.context("read WebDAV payload")? {
        writer
            .write_all(&chunk)
            .await
            .context("write WebDAV payload")?;
        let n = chunk.len() as u64;
        written = written.saturating_add(n);
        progress.advance(n);
    }
    Ok(written)
}

async fn try_delete_webdav(
    client: &crate::webdav::Client,
    object_key: &str,
    delete_after_recv: bool,
    trace: &mut RecvTrace,
) {
    if !delete_after_recv {
        return;
    }
    match client.delete(object_key).await {
        Ok(status) if (200..300).contains(&status) => {
            trace.info("webdav delete requested after receive")
        }
        Ok(404) => trace.info("webdav delete ignored: object already missing"),
        Ok(status) => trace.info(format_args!("webdav delete ignored: status {status}")),
        Err(err) => trace.info(format_args!("webdav delete ignored: {err:#}")),
    }
}
