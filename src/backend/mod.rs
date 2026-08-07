use crate::{command::RecvArgs, transport::source::checksum_path};
use anyhow::{Result, bail};
use std::path::PathBuf;

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

pub(crate) async fn report_checksum(args: &RecvArgs, path: PathBuf) -> Result<()> {
    let Some(algorithm) = args.checksum else {
        return Ok(());
    };
    let value = checksum_path(path, algorithm).await?;
    report_checksum_value(args.json, algorithm, &value);
    Ok(())
}

pub(crate) fn report_checksum_value(
    json: bool,
    algorithm: crate::command::ChecksumAlgorithm,
    value: &str,
) {
    if json {
        crate::json::emit(
            "checksum",
            &[
                ("operation", crate::json::Value::String("recv")),
                ("algorithm", crate::json::Value::String(algorithm.name())),
                ("value", crate::json::Value::String(value)),
            ],
        );
    } else {
        println!("checksum ({}): {}", algorithm.name(), value);
    }
}
