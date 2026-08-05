use anyhow::{Result, bail};

pub(crate) mod azure;
pub(crate) mod ftp;
pub(crate) mod s3;
pub(crate) mod sftp;
pub(crate) mod webdav;

pub(crate) fn remote_path_parts(path: &str) -> Result<Vec<&str>> {
    let parts = path
        .trim_matches('/')
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if parts.is_empty() || parts.iter().any(|part| *part == "." || *part == "..") {
        bail!("invalid remote object path {path}");
    }
    Ok(parts)
}
