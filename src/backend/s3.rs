use crate::{
    command::RecvArgs,
    ticket::{PayloadKind, Ticket},
    transport::p2p::{FilePlan, RecvTrace},
};
use crate::{
    storage,
    transport::{
        progress::TransferProgress,
        source::{Source, unique_object_id},
    },
};
use anyhow::{Context, Result, bail};
use std::io::{Read, Write};
use std::path::PathBuf;
use tempfile::NamedTempFile;
use tokio::fs;

use self::{ensure_success as ensure_s3_success, get as s3_get};

pub(crate) fn object_exists(bucket: &crate::s3::Client, object_path: &str) -> Result<bool> {
    match bucket.head_object(object_path) {
        Ok((_, code)) if (200..300).contains(&code) => Ok(true),
        Ok((_, 404)) => Ok(false),
        Ok((_, code)) => bail!("S3 object check failed with status {code}"),
        Err(_) => Ok(false),
    }
}

pub(crate) fn get(url: &str, resume_from: u64) -> Result<attohttpc::Response> {
    let mut request = attohttpc::get(url);
    if resume_from > 0 {
        request = request.header("range", format!("bytes={resume_from}-"));
    }
    request.send().context("download from S3")
}

pub(crate) fn ensure_success(status: u16) -> Result<()> {
    if (200..300).contains(&status) {
        Ok(())
    } else {
        bail!("S3 download failed with status {status}")
    }
}

pub(crate) struct ProgressReader<R> {
    inner: R,
    progress: TransferProgress,
}

impl<R> ProgressReader<R> {
    pub(crate) fn new(inner: R, progress: TransferProgress) -> Self {
        Self { inner, progress }
    }

    pub(crate) fn finish(&mut self) {
        self.progress.finish();
    }
}

impl<R: Read> Read for ProgressReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        if n > 0 {
            self.progress.advance(n as u64);
        }
        Ok(n)
    }
}

pub(crate) struct S3Upload {
    pub(crate) download_url: String,
    pub(crate) delete_url: Option<String>,
    pub(crate) object_key: String,
}

pub(crate) async fn upload(
    source: &Source,
    profile: &storage::S3Profile,
    delete_after_recv: bool,
    show_progress: bool,
) -> Result<S3Upload> {
    let source_path = source.local_path();
    let source_size = source.size();
    let profile = profile.clone();
    let object_key = match source.content_md5() {
        Some(content_md5) => storage::content_addressed_object_key(&profile.prefix, content_md5),
        None => storage::normalized_object_key(&profile.prefix, &unique_object_id(), source.name()),
    };
    let object_path = profile.s3_path(&object_key);
    tokio::task::spawn_blocking(move || -> Result<S3Upload> {
        let bucket = storage::build_bucket(&profile)?;
        if !object_exists(&bucket, &object_path)? {
            let file = std::fs::File::open(&source_path)
                .with_context(|| format!("open source file {}", source_path.display()))?;
            let progress = TransferProgress::new("ii send", show_progress, source_size, 0);
            let mut file = ProgressReader::new(file, progress);
            let status = bucket
                .put_object_stream(&mut file, &object_path)
                .context("upload to S3")?;
            if !(200..300).contains(&status) {
                bail!("S3 upload failed with status {status}");
            }
            file.finish();
        }
        let download_url = bucket
            .presign_get(&object_path, profile.presign_ttl_seconds, None)
            .context("create presigned download url")?;
        let delete_url = if delete_after_recv {
            Some(
                bucket
                    .presign_delete(&object_path, profile.presign_ttl_seconds)
                    .context("create presigned delete url")?,
            )
        } else {
            None
        };
        Ok(S3Upload {
            download_url,
            delete_url,
            object_key,
        })
    })
    .await
    .context("upload task")?
}

pub(crate) fn copy_blocking_with_progress<R, W>(
    reader: &mut R,
    writer: &mut W,
    progress: &mut TransferProgress,
) -> Result<u64>
where
    R: Read,
    W: Write,
{
    let mut buf = [0u8; 64 * 1024];
    let mut written = 0u64;
    loop {
        let n = reader.read(&mut buf).context("read payload")?;
        if n == 0 {
            break;
        }
        writer.write_all(&buf[..n]).context("write payload")?;
        let n = n as u64;
        written = written.saturating_add(n);
        progress.advance(n);
    }
    Ok(written)
}

pub(crate) async fn recv_s3(
    args: RecvArgs,
    ticket: Ticket,
    out_dir: PathBuf,
    file_target: Option<(PathBuf, FilePlan)>,
    mut trace: RecvTrace,
    show_progress: bool,
) -> Result<()> {
    let s3 = ticket
        .s3_route()
        .context("s3 ticket missing route")?
        .clone();
    trace.info("using s3 storage route");
    let bytes_written = match ticket.kind() {
        PayloadKind::File | PayloadKind::Stdin => {
            if args.stdout {
                download_s3_to_stdout(
                    s3.download_url.clone(),
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
                download_s3_to_file(
                    s3.download_url.clone(),
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
            download_s3_tar(
                s3.download_url.clone(),
                out_dir,
                ticket.size(),
                show_progress,
                &mut trace,
            )
            .await?
        }
    };
    trace.step("receive payload");
    trace.info(format_args!("received {} bytes", bytes_written));
    try_delete_s3(s3.delete_url.clone(), &mut trace).await;
    trace.finish(bytes_written);
    Ok(())
}

pub(crate) async fn try_delete_s3(delete_url: Option<String>, trace: &mut RecvTrace) {
    let Some(delete_url) = delete_url else {
        return;
    };
    let result = tokio::task::spawn_blocking(move || -> Result<()> {
        let response = attohttpc::delete(&delete_url)
            .send()
            .context("delete from S3")?;
        let status = response.status().as_u16();
        if (200..300).contains(&status) || status == 403 || status == 404 {
            Ok(())
        } else {
            bail!("delete returned status {status}");
        }
    })
    .await;
    match result {
        Ok(Ok(())) => trace.info("s3 delete requested after receive"),
        Ok(Err(err)) => trace.info(format_args!("s3 delete ignored: {err:#}")),
        Err(err) => trace.info(format_args!("s3 delete task failed: {err:#}")),
    }
}

async fn download_s3_to_file(
    url: String,
    path: PathBuf,
    resume_from: u64,
    total_size: Option<u64>,
    show_progress: bool,
    trace: &mut RecvTrace,
) -> Result<u64> {
    trace.info(format_args!("download s3 file to {}", path.display()));
    tokio::task::spawn_blocking(move || -> Result<u64> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let mut append = resume_from > 0;
        let mut response = s3_get(&url, resume_from)?;
        if resume_from > 0 && response.status().as_u16() == 200 {
            append = false;
            response = s3_get(&url, 0)?;
        }
        ensure_s3_success(response.status().as_u16())?;
        let mut file = if append {
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .with_context(|| format!("open destination {}", path.display()))?
        } else {
            std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&path)
                .with_context(|| format!("open destination {}", path.display()))?
        };
        let completed = if append { resume_from } else { 0 };
        let mut progress = TransferProgress::new("ii recv", show_progress, total_size, completed);
        let bytes = copy_blocking_with_progress(&mut response, &mut file, &mut progress)
            .with_context(|| format!("write destination {}", path.display()))?;
        progress.finish();
        file.flush()
            .with_context(|| format!("flush destination {}", path.display()))?;
        Ok(bytes)
    })
    .await
    .context("s3 download task")?
}

async fn download_s3_to_stdout(
    url: String,
    total_size: Option<u64>,
    show_progress: bool,
    trace: &mut RecvTrace,
) -> Result<u64> {
    trace.info("download s3 file to stdout");
    tokio::task::spawn_blocking(move || -> Result<u64> {
        let mut response = s3_get(&url, 0)?;
        ensure_s3_success(response.status().as_u16())?;
        let mut stdout = std::io::stdout();
        let mut progress = TransferProgress::new("ii recv", show_progress, total_size, 0);
        let bytes = copy_blocking_with_progress(&mut response, &mut stdout, &mut progress)
            .context("write stdout")?;
        progress.finish();
        stdout.flush().ok();
        Ok(bytes)
    })
    .await
    .context("s3 stdout task")?
}

async fn download_s3_tar(
    url: String,
    out_dir: PathBuf,
    total_size: Option<u64>,
    show_progress: bool,
    trace: &mut RecvTrace,
) -> Result<u64> {
    trace.info(format_args!("download s3 tar to {}", out_dir.display()));
    fs::create_dir_all(&out_dir)
        .await
        .with_context(|| format!("create output dir {}", out_dir.display()))?;
    let temp = NamedTempFile::new().context("create temp tar")?;
    let temp_path = temp.path().to_path_buf();
    let url_for_task = url.clone();
    let bytes = tokio::task::spawn_blocking(move || -> Result<u64> {
        let mut response = s3_get(&url_for_task, 0)?;
        ensure_s3_success(response.status().as_u16())?;
        let mut file = std::fs::File::create(&temp_path).context("create temp tar destination")?;
        let mut progress = TransferProgress::new("ii recv", show_progress, total_size, 0);
        let bytes = copy_blocking_with_progress(&mut response, &mut file, &mut progress)
            .context("buffer s3 tar")?;
        progress.finish();
        file.flush().context("flush temp tar")?;
        Ok(bytes)
    })
    .await
    .context("s3 tar download task")??;

    let extract_path = out_dir.clone();
    let temp_path = temp.path().to_path_buf();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let file = std::fs::File::open(&temp_path).context("open tar")?;
        let mut archive = tar::Archive::new(file);
        archive.unpack(&extract_path).context("unpack tar")?;
        Ok(())
    })
    .await
    .context("extract s3 tar task")??;
    Ok(bytes)
}
