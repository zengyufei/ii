use crate::{
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

    pub(crate) async fn open_paths(
        path: Option<PathBuf>,
        extra_paths: &[PathBuf],
        override_name: Option<String>,
        includes: &[String],
        excludes: &[String],
    ) -> Result<Self> {
        if extra_paths.is_empty() {
            if includes.is_empty() && excludes.is_empty() {
                return Self::open(path, override_name).await;
            }
            return match path {
                None => Self::from_stdin(override_name).await,
                Some(path) if path.is_dir() => {
                    Self::from_archive(
                        vec![path],
                        override_name,
                        FilterSet::new(includes, excludes)?,
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

    async fn from_archive(
        paths: Vec<PathBuf>,
        root_name: Option<String>,
        filters: FilterSet,
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
) -> Result<bool> {
    let metadata = std::fs::metadata(source_path)
        .with_context(|| format!("read source {}", source_path.display()))?;
    if metadata.is_dir() {
        if filters.excluded(filter_path) {
            return Ok(false);
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
            )?;
        }
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
}
