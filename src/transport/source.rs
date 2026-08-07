use crate::{
    command::{ChecksumAlgorithm, SymlinkPolicy},
    ticket::PayloadKind,
    transport::progress::{RateLimiter, TransferProgress, copy_with_progress_limited},
};
use anyhow::{Context, Result, bail};
use glob::{MatchOptions, Pattern};
use std::{
    ffi::OsStr,
    io::{IsTerminal, Read},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    task::{Context as TaskContext, Poll},
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

pub(crate) struct ChecksumWriter<W> {
    inner: W,
    algorithm: ChecksumAlgorithm,
    md5: md5::Md5,
    sha256: sha2::Sha256,
}

impl<W> ChecksumWriter<W> {
    pub(crate) fn new(inner: W, algorithm: ChecksumAlgorithm) -> Self {
        Self {
            inner,
            algorithm,
            md5: <md5::Md5 as md5::Digest>::new(),
            sha256: <sha2::Sha256 as sha2::Digest>::new(),
        }
    }

    pub(crate) fn finish(self) -> String {
        let bytes: Vec<u8> = match self.algorithm {
            ChecksumAlgorithm::Md5 => md5::Digest::finalize(self.md5).to_vec(),
            ChecksumAlgorithm::Sha256 => sha2::Digest::finalize(self.sha256).to_vec(),
        };
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

impl<W: AsyncWrite + Unpin> AsyncWrite for ChecksumWriter<W> {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match std::pin::Pin::new(&mut self.inner).poll_write(cx, buf) {
            Poll::Ready(Ok(written)) => {
                match self.algorithm {
                    ChecksumAlgorithm::Md5 => md5::Digest::update(&mut self.md5, &buf[..written]),
                    ChecksumAlgorithm::Sha256 => {
                        sha2::Digest::update(&mut self.sha256, &buf[..written])
                    }
                }
                Poll::Ready(Ok(written))
            }
            other => other,
        }
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
    ) -> Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
    ) -> Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

impl Source {
    pub(crate) async fn open_with_options(
        path: Option<PathBuf>,
        override_name: Option<String>,
        symlinks: SymlinkPolicy,
        preserve_metadata: bool,
    ) -> Result<Self> {
        match path {
            None => Self::from_stdin(override_name).await,
            Some(path) if preserve_metadata => {
                let metadata = fs::symlink_metadata(&path)
                    .await
                    .with_context(|| format!("stat source file {}", path.display()))?;
                if !metadata.file_type().is_file() {
                    bail!("--preserve-metadata requires a regular file path");
                }
                Self::from_single_file_archive(path, override_name, symlinks).await
            }
            Some(path)
                if symlinks != SymlinkPolicy::Follow
                    && fs::symlink_metadata(&path)
                        .await
                        .map(|metadata| metadata.file_type().is_symlink())
                        .unwrap_or(false) =>
            {
                Self::from_single_file_archive(path, override_name, symlinks).await
            }
            Some(path) if path.is_dir() => Self::from_dir(path, override_name, symlinks).await,
            Some(path) => Self::from_file(path, override_name).await,
        }
    }

    #[cfg(test)]
    pub(crate) async fn open_paths(
        path: Option<PathBuf>,
        extra_paths: &[PathBuf],
        override_name: Option<String>,
        includes: &[String],
        excludes: &[String],
    ) -> Result<Self> {
        Self::open_paths_with_options(
            path,
            extra_paths,
            override_name,
            includes,
            excludes,
            SymlinkPolicy::Follow,
            false,
        )
        .await
    }

    pub(crate) async fn open_paths_with_options(
        path: Option<PathBuf>,
        extra_paths: &[PathBuf],
        override_name: Option<String>,
        includes: &[String],
        excludes: &[String],
        symlinks: SymlinkPolicy,
        preserve_metadata: bool,
    ) -> Result<Self> {
        if extra_paths.is_empty() {
            if includes.is_empty() && excludes.is_empty() {
                return Self::open_with_options(path, override_name, symlinks, preserve_metadata)
                    .await;
            }
            return match path {
                None => Self::from_stdin(override_name).await,
                Some(path) if path.is_dir() => {
                    Self::from_archive(
                        vec![path],
                        override_name,
                        FilterSet::new(includes, excludes)?,
                        symlinks,
                    )
                    .await
                }
                Some(path) => Self::from_file(path, override_name).await,
            };
        }

        let Some(path) = path else {
            bail!("cannot combine stdin with file paths");
        };
        let mut paths = Vec::with_capacity(extra_paths.len() + 1);
        paths.push(path);
        paths.extend(extra_paths.iter().cloned());
        Self::from_archive(
            paths,
            Some(override_name.unwrap_or_else(|| "ii".to_string())),
            FilterSet::new(includes, excludes)?,
            symlinks,
        )
        .await
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

    async fn from_single_file_archive(
        path: PathBuf,
        override_name: Option<String>,
        symlinks: SymlinkPolicy,
    ) -> Result<Self> {
        let name = override_name.unwrap_or_else(|| {
            path.file_name()
                .and_then(OsStr::to_str)
                .unwrap_or("ii-file")
                .to_string()
        });
        let temp = NamedTempFile::new().context("create temp archive")?;
        let archive_path = temp.path().to_path_buf();
        let archive_path_for_task = archive_path.clone();
        let archive_name = name.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let file = std::fs::File::create(&archive_path_for_task).context("create archive")?;
            let mut builder = tar::Builder::new(file);
            let metadata = std::fs::symlink_metadata(&path).context("read source metadata")?;
            if metadata.file_type().is_symlink() {
                if symlinks == SymlinkPolicy::Reject {
                    bail!("symbolic link is not allowed: {}", path.display());
                }
                let target = std::fs::read_link(&path).context("read symbolic link")?;
                let mut header = tar::Header::new_gnu();
                header.set_metadata(&metadata);
                header.set_entry_type(tar::EntryType::Symlink);
                header.set_size(0);
                builder
                    .append_link(&mut header, &archive_name, target)
                    .context("archive symbolic link")?;
            } else {
                builder
                    .append_path_with_name(&path, &archive_name)
                    .context("archive source file")?;
            }
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

    async fn from_dir(
        path: PathBuf,
        override_name: Option<String>,
        symlinks: SymlinkPolicy,
    ) -> Result<Self> {
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
            let filters = FilterSet::new(&[], &[])?;
            let mut visited = std::collections::BTreeSet::new();
            append_filtered_path(
                &mut builder,
                &src_path,
                Path::new(&archive_name),
                Path::new(""),
                &filters,
                symlinks,
                &mut visited,
            )?;
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

    async fn from_archive(
        paths: Vec<PathBuf>,
        root_name: Option<String>,
        filters: FilterSet,
        symlinks: SymlinkPolicy,
    ) -> Result<Self> {
        let root_name = root_name.unwrap_or_else(|| {
            paths
                .first()
                .and_then(|path| path.file_name())
                .and_then(OsStr::to_str)
                .unwrap_or("ii-dir")
                .to_string()
        });
        let temp = NamedTempFile::new().context("create temp archive")?;
        let archive_path = temp.path().to_path_buf();
        let archive_path_for_task = archive_path.clone();
        let archive_root = root_name.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let file = std::fs::File::create(&archive_path_for_task).context("create archive")?;
            let mut builder = tar::Builder::new(file);
            let mut seen = std::collections::BTreeSet::new();
            let mut appended = false;
            let mut visited = std::collections::BTreeSet::new();
            for path in paths {
                let name = archive_entry_name(&path)?;
                if !seen.insert(name.clone()) {
                    bail!("multiple inputs have the same top-level name `{name}`");
                }
                let filter_path = if path.is_dir() {
                    PathBuf::new()
                } else {
                    PathBuf::from(&name)
                };
                appended |= append_filtered_path(
                    &mut builder,
                    &path,
                    &PathBuf::from(&archive_root).join(&name),
                    &filter_path,
                    &filters,
                    symlinks,
                    &mut visited,
                )?;
            }
            if !appended {
                bail!("no files matched --include/--exclude");
            }
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
            name: root_name,
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

    pub(crate) async fn checksum(&self, algorithm: ChecksumAlgorithm) -> Result<String> {
        checksum_path(self.local_path(), algorithm).await
    }

    pub(crate) async fn stream_to_limited<W: AsyncWrite + Unpin>(
        &self,
        out: &mut W,
        resume_from: u64,
        show_progress: bool,
        rate_limiter: Option<&Arc<RateLimiter>>,
    ) -> Result<()> {
        self.stream_to_with_progress(
            out,
            resume_from,
            TransferProgress::new("ii send", show_progress, self.size(), resume_from),
            rate_limiter.map(Arc::as_ref),
        )
        .await
    }

    pub(crate) async fn stream_to_multiline_limited<W: AsyncWrite + Unpin>(
        &self,
        out: &mut W,
        resume_from: u64,
        show_progress: bool,
        label: String,
        rate_limiter: Option<&Arc<RateLimiter>>,
    ) -> Result<()> {
        self.stream_to_with_progress(
            out,
            resume_from,
            TransferProgress::multiline(label, show_progress, self.size(), resume_from),
            rate_limiter.map(Arc::as_ref),
        )
        .await
    }

    async fn stream_to_with_progress<W: AsyncWrite + Unpin>(
        &self,
        out: &mut W,
        resume_from: u64,
        mut progress: TransferProgress,
        rate_limiter: Option<&RateLimiter>,
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
        copy_with_progress_limited(&mut file, out, &mut progress, rate_limiter)
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

#[derive(Clone)]
struct FilterSet {
    includes: Vec<Pattern>,
    excludes: Vec<Pattern>,
}

impl FilterSet {
    fn new(includes: &[String], excludes: &[String]) -> Result<Self> {
        Ok(Self {
            includes: includes
                .iter()
                .map(|value| Pattern::new(value).context("parse --include glob"))
                .collect::<Result<_>>()?,
            excludes: excludes
                .iter()
                .map(|value| Pattern::new(value).context("parse --exclude glob"))
                .collect::<Result<_>>()?,
        })
    }

    fn selected(&self, path: &Path) -> bool {
        let path = path.to_string_lossy().replace('\\', "/");
        let options = MatchOptions {
            case_sensitive: true,
            require_literal_separator: true,
            require_literal_leading_dot: false,
        };
        !self
            .excludes
            .iter()
            .any(|pattern| pattern.matches_with(&path, options))
            && (self.includes.is_empty()
                || self
                    .includes
                    .iter()
                    .any(|pattern| pattern.matches_with(&path, options)))
    }

    fn excluded(&self, path: &Path) -> bool {
        let path = path.to_string_lossy().replace('\\', "/");
        let options = MatchOptions {
            case_sensitive: true,
            require_literal_separator: true,
            require_literal_leading_dot: false,
        };
        self.excludes
            .iter()
            .any(|pattern| pattern.matches_with(&path, options))
    }
}

fn archive_entry_name(path: &Path) -> Result<String> {
    path.file_name()
        .filter(|name| !name.is_empty())
        .map(OsStr::to_os_string)
        .or_else(|| {
            std::fs::canonicalize(path)
                .ok()
                .and_then(|path| path.file_name().map(OsStr::to_os_string))
        })
        .and_then(|name| name.to_str().map(str::to_string))
        .filter(|name| !name.is_empty())
        .context("derive source file name")
}

fn append_filtered_path(
    builder: &mut tar::Builder<std::fs::File>,
    source_path: &Path,
    archive_path: &Path,
    filter_path: &Path,
    filters: &FilterSet,
    symlinks: SymlinkPolicy,
    visited: &mut std::collections::BTreeSet<PathBuf>,
) -> Result<bool> {
    let link_metadata = std::fs::symlink_metadata(source_path)
        .with_context(|| format!("read source {}", source_path.display()))?;
    if link_metadata.file_type().is_symlink() {
        match symlinks {
            SymlinkPolicy::Reject => {
                bail!("symbolic link is not allowed: {}", source_path.display())
            }
            SymlinkPolicy::Preserve => {
                if !filters.selected(filter_path) {
                    return Ok(false);
                }
                let target = std::fs::read_link(source_path)
                    .with_context(|| format!("read symbolic link {}", source_path.display()))?;
                let mut header = tar::Header::new_gnu();
                header.set_metadata(&link_metadata);
                header.set_entry_type(tar::EntryType::Symlink);
                header.set_size(0);
                builder
                    .append_link(&mut header, archive_path, target)
                    .with_context(|| format!("archive symbolic link {}", source_path.display()))?;
                return Ok(true);
            }
            SymlinkPolicy::Follow => {}
        }
    }

    let metadata = std::fs::metadata(source_path)
        .with_context(|| format!("read source {}", source_path.display()))?;
    if metadata.is_dir() {
        if filters.excluded(filter_path) {
            return Ok(false);
        }
        let canonical = std::fs::canonicalize(source_path)
            .with_context(|| format!("resolve source directory {}", source_path.display()))?;
        if !visited.insert(canonical) {
            bail!("symbolic link directory cycle at {}", source_path.display());
        }
        let mut appended = filters.selected(filter_path);
        let mut entries = std::fs::read_dir(source_path)
            .with_context(|| format!("read source directory {}", source_path.display()))?
            .collect::<std::io::Result<Vec<_>>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            appended |= append_filtered_path(
                builder,
                &entry.path(),
                &archive_path.join(entry.file_name()),
                &filter_path.join(entry.file_name()),
                filters,
                symlinks,
                visited,
            )?;
        }
        visited.remove(
            &std::fs::canonicalize(source_path)
                .with_context(|| format!("resolve source directory {}", source_path.display()))?,
        );
        if appended {
            builder
                .append_dir(archive_path, source_path)
                .with_context(|| format!("archive directory {}", source_path.display()))?;
        }
        Ok(appended)
    } else if filters.selected(filter_path) {
        builder
            .append_path_with_name(source_path, archive_path)
            .with_context(|| format!("archive source {}", source_path.display()))?;
        Ok(true)
    } else {
        Ok(false)
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

pub(crate) async fn checksum_path(path: PathBuf, algorithm: ChecksumAlgorithm) -> Result<String> {
    tokio::task::spawn_blocking(move || checksum_path_blocking(&path, algorithm))
        .await
        .context("checksum task")?
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

fn checksum_path_blocking(path: &Path, algorithm: ChecksumAlgorithm) -> Result<String> {
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("open file for {} {}", algorithm.name(), path.display()))?;
    let mut buf = [0u8; 64 * 1024];
    let mut md5 = <md5::Md5 as md5::Digest>::new();
    let mut sha256 = <sha2::Sha256 as sha2::Digest>::new();
    loop {
        let n = file
            .read(&mut buf)
            .with_context(|| format!("read file for {} {}", algorithm.name(), path.display()))?;
        if n == 0 {
            break;
        }
        match algorithm {
            ChecksumAlgorithm::Md5 => md5::Digest::update(&mut md5, &buf[..n]),
            ChecksumAlgorithm::Sha256 => sha2::Digest::update(&mut sha256, &buf[..n]),
        }
    }
    let bytes: Vec<u8> = match algorithm {
        ChecksumAlgorithm::Md5 => md5::Digest::finalize(md5).to_vec(),
        ChecksumAlgorithm::Sha256 => sha2::Digest::finalize(sha256).to_vec(),
    };
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn archive_names(source: &Source) -> Vec<String> {
        let file = std::fs::File::open(source.local_path()).unwrap();
        let mut archive = tar::Archive::new(file);
        archive
            .entries()
            .unwrap()
            .map(|entry| {
                entry
                    .unwrap()
                    .path()
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect()
    }

    #[tokio::test]
    async fn multiple_paths_use_the_ii_collection_root() {
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("first.txt");
        let second = temp.path().join("second.txt");
        std::fs::write(&first, b"first").unwrap();
        std::fs::write(&second, b"second").unwrap();

        let source = Source::open_paths(Some(first), &[second], None, &[], &[])
            .await
            .unwrap();

        assert_eq!(source.kind(), PayloadKind::Dir);
        assert_eq!(source.name(), "ii");
        assert_eq!(archive_names(&source), ["ii/first.txt", "ii/second.txt"]);
    }

    #[tokio::test]
    async fn filters_match_paths_relative_to_each_input_directory() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("source");
        std::fs::create_dir_all(directory.join("nested")).unwrap();
        std::fs::write(directory.join("keep.txt"), b"keep").unwrap();
        std::fs::write(directory.join("nested").join("drop.log"), b"drop").unwrap();

        let source = Source::open_paths(
            Some(directory),
            &[],
            Some("collection".to_string()),
            &["keep.txt".to_string()],
            &[],
        )
        .await
        .unwrap();

        let names = archive_names(&source);
        assert!(names.contains(&"collection/source/keep.txt".to_string()));
        assert!(!names.iter().any(|name| name.ends_with("drop.log")));
    }

    #[tokio::test]
    async fn duplicate_top_level_names_and_empty_filters_fail() {
        let temp = tempfile::tempdir().unwrap();
        let left = temp.path().join("left");
        let right = temp.path().join("right");
        std::fs::create_dir_all(&left).unwrap();
        std::fs::create_dir_all(&right).unwrap();
        let first = left.join("same.txt");
        let second = right.join("same.txt");
        std::fs::write(&first, b"first").unwrap();
        std::fs::write(&second, b"second").unwrap();

        assert!(
            Source::open_paths(Some(first.clone()), &[second], None, &[], &[])
                .await
                .is_err()
        );
        assert!(
            Source::open_paths(
                Some(left),
                &[],
                Some("collection".to_string()),
                &["missing/**".to_string()],
                &[],
            )
            .await
            .is_err()
        );
    }

    #[tokio::test]
    async fn preserve_metadata_wraps_a_single_file_as_tar() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("note.txt");
        std::fs::write(&path, b"hello").unwrap();
        let source = Source::open_with_options(Some(path), None, SymlinkPolicy::Follow, true)
            .await
            .unwrap();
        assert_eq!(source.kind(), PayloadKind::Dir);
        assert_eq!(archive_names(&source), ["note.txt"]);
    }

    #[tokio::test]
    async fn checksum_writer_hashes_only_written_bytes() {
        let mut md5_writer = ChecksumWriter::new(tokio::io::sink(), ChecksumAlgorithm::Md5);
        md5_writer.write_all(b"abc").await.unwrap();
        assert_eq!(md5_writer.finish(), "900150983cd24fb0d6963f7d28e17f72");

        let mut sha256_writer = ChecksumWriter::new(tokio::io::sink(), ChecksumAlgorithm::Sha256);
        sha256_writer.write_all(b"abc").await.unwrap();
        assert_eq!(
            sha256_writer.finish(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn symlink_preserve_keeps_link_and_reject_fails() {
        use std::os::unix::fs::symlink;

        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("target.txt");
        let link = temp.path().join("link.txt");
        std::fs::write(&target, b"target").unwrap();
        symlink(&target, &link).unwrap();

        let preserved =
            Source::open_with_options(Some(link.clone()), None, SymlinkPolicy::Preserve, false)
                .await
                .unwrap();
        assert_eq!(archive_names(&preserved), ["link.txt"]);
        let mut archive = tar::Archive::new(std::fs::File::open(preserved.local_path()).unwrap());
        let entry = archive.entries().unwrap().next().unwrap().unwrap();
        assert!(entry.header().entry_type().is_symlink());

        assert!(
            Source::open_with_options(Some(link.clone()), None, SymlinkPolicy::Follow, true,)
                .await
                .is_err()
        );
        assert!(
            Source::open_with_options(Some(link), None, SymlinkPolicy::Reject, false)
                .await
                .is_err()
        );
    }
}
