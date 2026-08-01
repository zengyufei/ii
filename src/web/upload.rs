use crate::web::http::{write_web_error, write_web_response};
use anyhow::{Context, Result, bail};
use percent_encoding::percent_decode_str;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;
use tokio::{
    fs,
    io::{self, AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

pub(crate) fn name(target: &str) -> Result<String> {
    let encoded = target
        .strip_prefix("upload?name=")
        .context("upload name is missing")?;
    if encoded.is_empty() || encoded.contains('&') {
        bail!("upload name is invalid");
    }
    let name = percent_decode_str(encoded)
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
    if let Err(err) = fs::remove_file(&path).await
        && err.kind() != std::io::ErrorKind::NotFound
    {
        let message = format!("replace upload file: {err}");
        return write_web_error(stream, "500 Internal Server Error", &message).await;
    }
    if let Err(err) = temp.persist(&path) {
        let message = format!("replace upload file: {}", err.error);
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
