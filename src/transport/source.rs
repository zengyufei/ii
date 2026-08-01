use crate::{
    ticket::PayloadKind,
    transport::progress::{TransferProgress, copy_with_progress},
};
use anyhow::{Context, Result, bail};
use std::{
    ffi::OsStr,
    io::{IsTerminal, Read},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
use tempfile::NamedTempFile;
use tokio::{
    fs,
    io::{self, AsyncSeekExt, AsyncWrite, AsyncWriteExt},
};

enum Backing {
    Path(PathBuf),
    Temp(NamedTempFile),
}

pub(crate) struct Source {
    backing: Backing,
    name: String,
    kind: PayloadKind,
    pub(crate) size: u64,
    content_md5: Option<[u8; 16]>,
}

impl Source {
    pub(crate) async fn open(path: Option<PathBuf>, override_name: Option<String>) -> Result<Self> {
        match path {
            None => Self::from_stdin(override_name).await,
            Some(path) if path.is_dir() => Self::from_dir(path, override_name).await,
            Some(path) => Self::from_file(path, override_name).await,
        }
    }

    async fn from_stdin(override_name: Option<String>) -> Result<Self> {
        if std::io::stdin().is_terminal() {
            bail!("no path provided and stdin is interactive");
        }
        let name = override_name.unwrap_or_else(|| "stdin".to_string());
        let temp = NamedTempFile::new().context("create temp file")?;
        let path = temp.path().to_path_buf();
        let mut file = fs::File::from_std(temp.reopen().context("reopen temp file")?);
        let mut stdin = tokio::io::stdin();
        io::copy(&mut stdin, &mut file)
            .await
            .context("read stdin")?;
        file.flush().await.context("flush stdin temp file")?;
        let size = fs::metadata(&path)
            .await
            .context("stat stdin temp file")?
            .len();
        let content_md5 = md5_path(path).await?;
        Ok(Self {
            backing: Backing::Temp(temp),
            name,
            kind: PayloadKind::Stdin,
            size,
            content_md5: Some(content_md5),
        })
    }

    pub(crate) async fn from_file(path: PathBuf, override_name: Option<String>) -> Result<Self> {
        let meta = fs::metadata(&path).await.context("stat source file")?;
        let name = override_name.unwrap_or_else(|| {
            path.file_name()
                .and_then(OsStr::to_str)
                .unwrap_or("ii-file")
                .to_string()
        });
        let content_md5 = md5_path(path.clone()).await?;
        Ok(Self {
            backing: Backing::Path(path),
            name,
            kind: PayloadKind::File,
            size: meta.len(),
            content_md5: Some(content_md5),
        })
    }

    async fn from_dir(path: PathBuf, override_name: Option<String>) -> Result<Self> {
        let name = override_name.unwrap_or_else(|| {
            path.file_name()
                .and_then(OsStr::to_str)
                .unwrap_or("ii-dir")
                .to_string()
        });
        let temp = NamedTempFile::new().context("create temp archive")?;
        let archive_path = temp.path().to_path_buf();
        let src_path = path.clone();
        let archive_name = name.clone();
        let archive_path_for_task = archive_path.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let file = std::fs::File::create(&archive_path_for_task).context("create archive")?;
            let mut builder = tar::Builder::new(file);
            builder
                .append_dir_all(&archive_name, &src_path)
                .context("build tar archive")?;
            builder.finish().context("finish tar archive")?;
            Ok(())
        })
        .await
        .context("archive task")??;
        let size = std::fs::metadata(&archive_path)
            .context("stat tar archive")?
            .len();
        Ok(Self {
            backing: Backing::Temp(temp),
            name,
            kind: PayloadKind::Dir,
            size,
            content_md5: None,
        })
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) fn kind(&self) -> PayloadKind {
        self.kind
    }

    pub(crate) fn size(&self) -> Option<u64> {
        Some(self.size)
    }

    pub(crate) fn content_md5(&self) -> Option<[u8; 16]> {
        self.content_md5
    }

    pub(crate) fn local_path(&self) -> PathBuf {
        match &self.backing {
            Backing::Path(path) => path.clone(),
            Backing::Temp(temp) => temp.path().to_path_buf(),
        }
    }

    pub(crate) async fn stream_to<W: AsyncWrite + Unpin>(
        &self,
        out: &mut W,
        resume_from: u64,
        show_progress: bool,
    ) -> Result<()> {
        if resume_from > 0 && self.kind == PayloadKind::Dir {
            bail!("resume is only supported for regular files");
        }
        let mut file = self.open_file().await?;
        if resume_from > 0 {
            file.seek(std::io::SeekFrom::Start(resume_from))
                .await
                .context("seek resume offset")?;
        }
        let mut progress =
            TransferProgress::new("ii send", show_progress, self.size(), resume_from);
        copy_with_progress(&mut file, out, &mut progress)
            .await
            .context("stream payload")?;
        progress.finish();
        Ok(())
    }

    pub(crate) async fn open_file(&self) -> Result<fs::File> {
        match &self.backing {
            Backing::Path(path) => fs::File::open(path).await.context("open source file"),
            Backing::Temp(temp) => fs::File::open(temp.path())
                .await
                .context("open temp source"),
        }
    }
}

static NEXT_OBJECT_ID: AtomicU64 = AtomicU64::new(0);

pub(crate) fn unique_object_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or_default();
    let sequence = NEXT_OBJECT_ID.fetch_add(1, Ordering::Relaxed) as u32;
    format!("{nanos:016x}{:08x}{sequence:08x}", std::process::id())
}

pub(crate) async fn md5_path(path: PathBuf) -> Result<[u8; 16]> {
    tokio::task::spawn_blocking(move || md5_path_blocking(&path))
        .await
        .context("hash task")?
}

fn md5_path_blocking(path: &Path) -> Result<[u8; 16]> {
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("open file for md5 {}", path.display()))?;
    let mut ctx = <md5::Md5 as md5::Digest>::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .with_context(|| format!("read file for md5 {}", path.display()))?;
        if n == 0 {
            break;
        }
        md5::Digest::update(&mut ctx, &buf[..n]);
    }
    Ok(finalize_md5(ctx))
}

#[cfg(test)]
pub(crate) fn md5_bytes(bytes: &[u8]) -> [u8; 16] {
    let mut ctx = <md5::Md5 as md5::Digest>::new();
    md5::Digest::update(&mut ctx, bytes);
    finalize_md5(ctx)
}

fn finalize_md5(ctx: md5::Md5) -> [u8; 16] {
    let digest = md5::Digest::finalize(ctx);
    let mut out = [0u8; 16];
    out.copy_from_slice(&digest);
    out
}
