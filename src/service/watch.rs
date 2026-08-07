use crate::{
    command::{SymlinkPolicy, WatchArgs},
    service::send,
};
use anyhow::{Context, Result};
use glob::{MatchOptions, Pattern};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::{Path, PathBuf},
    time::SystemTime,
};
use tokio::time::sleep;

#[derive(Clone, Debug, PartialEq, Eq)]
struct FileStamp {
    size: u64,
    modified: SystemTime,
}

pub(super) async fn run(args: WatchArgs) -> Result<()> {
    let root = tokio::fs::canonicalize(&args.dir)
        .await
        .with_context(|| format!("read watch directory {}", args.dir.display()))?;
    if !tokio::fs::metadata(&root).await?.is_dir() {
        anyhow::bail!("watch path is not a directory: {}", args.dir.display());
    }
    let filters = WatchFilters::new(&args.send.include, &args.send.exclude)?;
    let mut seen = scan_files(root.clone(), args.send.symlinks, filters.clone()).await?;
    let mut pending = BTreeMap::<PathBuf, (FileStamp, tokio::time::Instant)>::new();
    let mut jobs = VecDeque::new();
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => return Ok(()),
            _ = sleep(args.interval) => {}
        }
        let current = scan_files(root.clone(), args.send.symlinks, filters.clone()).await?;
        for (path, stamp) in &current {
            match seen.get(path) {
                Some(previous) if previous == stamp => {}
                _ => {
                    let entry = pending
                        .entry(path.clone())
                        .or_insert_with(|| (stamp.clone(), tokio::time::Instant::now()));
                    if entry.0 != *stamp {
                        *entry = (stamp.clone(), tokio::time::Instant::now());
                    }
                    if entry.1.elapsed() >= args.stabilize {
                        jobs.push_back(path.clone());
                        seen.insert(path.clone(), stamp.clone());
                        pending.remove(path);
                    }
                }
            }
        }
        pending.retain(|path, _| current.contains_key(path));
        seen.retain(|path, _| current.contains_key(path));
        while let Some(path) = jobs.pop_front() {
            let mut send_args = args.send.clone();
            send_args.path = Some(path.clone());
            send_args.extra_paths.clear();
            tokio::select! {
                result = send(send_args) => {
                    if let Err(err) = result {
                        eprintln!("ii watch: {} failed: {err:#}", path.display());
                    }
                }
                _ = tokio::signal::ctrl_c() => return Ok(()),
            }
        }
    }
}

#[derive(Clone)]
struct WatchFilters {
    includes: Vec<Pattern>,
    excludes: Vec<Pattern>,
}

impl WatchFilters {
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

    fn selected(&self, root: &Path, path: &Path) -> bool {
        let Ok(relative) = path.strip_prefix(root) else {
            return false;
        };
        let path = relative.to_string_lossy().replace('\\', "/");
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
}

async fn scan_files(
    root: PathBuf,
    policy: SymlinkPolicy,
    filters: WatchFilters,
) -> Result<BTreeMap<PathBuf, FileStamp>> {
    tokio::task::spawn_blocking(move || scan_files_blocking(&root, policy, &filters))
        .await
        .context("scan watch directory")?
}

fn scan_files_blocking(
    root: &Path,
    policy: SymlinkPolicy,
    filters: &WatchFilters,
) -> Result<BTreeMap<PathBuf, FileStamp>> {
    let mut out = BTreeMap::new();
    let mut visited = BTreeSet::new();
    scan_dir(root, root, policy, filters, &mut visited, &mut out)?;
    Ok(out)
}

fn scan_dir(
    root: &Path,
    directory: &Path,
    policy: SymlinkPolicy,
    filters: &WatchFilters,
    visited: &mut BTreeSet<PathBuf>,
    out: &mut BTreeMap<PathBuf, FileStamp>,
) -> Result<()> {
    let canonical = std::fs::canonicalize(directory)?;
    if !visited.insert(canonical) {
        return Ok(());
    }
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let link_meta = std::fs::symlink_metadata(&path)?;
        if link_meta.file_type().is_symlink() && policy == SymlinkPolicy::Reject {
            out.insert(
                path,
                FileStamp {
                    size: 0,
                    modified: link_meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                },
            );
            continue;
        }
        let meta = match policy {
            SymlinkPolicy::Preserve if link_meta.file_type().is_symlink() => link_meta.clone(),
            _ => std::fs::metadata(&path)?,
        };
        if meta.is_dir() {
            scan_dir(root, &path, policy, filters, visited, out)?;
        } else if (meta.is_file() || link_meta.file_type().is_symlink())
            && filters.selected(root, &path)
        {
            out.insert(
                path,
                FileStamp {
                    size: meta.len(),
                    modified: meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                },
            );
        }
    }
    visited.remove(&std::fs::canonicalize(directory)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_use_root_relative_slash_paths_and_exclude_wins() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        std::fs::create_dir(root.join("nested")).unwrap();
        std::fs::write(root.join("nested").join("keep.txt"), b"keep").unwrap();
        std::fs::write(root.join("nested").join("drop.txt"), b"drop").unwrap();
        std::fs::write(root.join("root.bin"), b"root").unwrap();
        let filters = WatchFilters::new(
            &["nested/*.txt".to_string()],
            &["nested/drop.*".to_string()],
        )
        .unwrap();

        let files = scan_files_blocking(root, SymlinkPolicy::Follow, &filters).unwrap();
        assert!(files.contains_key(&root.join("nested").join("keep.txt")));
        assert!(!files.contains_key(&root.join("nested").join("drop.txt")));
        assert!(!files.contains_key(&root.join("root.bin")));
    }
}
