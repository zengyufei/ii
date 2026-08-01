use crate::transport::progress::{TransferProgress, copy_with_progress};
use anyhow::{Context, Result};
use std::path::PathBuf;
use tempfile::NamedTempFile;
use tokio::{fs, io::AsyncWriteExt};

pub(crate) async fn extract_tar_stream(
    mut recv: iroh::endpoint::RecvStream,
    path: PathBuf,
    total_size: Option<u64>,
    show_progress: bool,
) -> Result<u64> {
    fs::create_dir_all(&path)
        .await
        .with_context(|| format!("create output dir {}", path.display()))?;
    let temp = NamedTempFile::new().context("create temp tar")?;
    let temp_path = temp.path().to_path_buf();
    let mut file = fs::File::from_std(temp.reopen().context("reopen temp tar")?);
    let mut progress = TransferProgress::new("ii recv", show_progress, total_size, 0);
    let bytes = copy_with_progress(&mut recv, &mut file, &mut progress)
        .await
        .context("buffer tar")?;
    progress.finish();
    file.flush().await.context("flush tar")?;
    let extract_path = path.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let file = std::fs::File::open(&temp_path).context("open tar")?;
        let mut archive = tar::Archive::new(file);
        archive.unpack(&extract_path).context("unpack tar")?;
        Ok(())
    })
    .await
    .context("extract task")??;
    Ok(bytes)
}
