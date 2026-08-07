use crate::{
    backend::remote_path_parts,
    command::RecvArgs,
    storage,
    ticket::{FtpPortableCredentials, PayloadKind, Ticket},
    transport::{
        p2p::{FilePlan, RecvTrace},
        progress::{RateLimiter, TransferProgress, copy_with_progress, copy_with_progress_limited},
        source::{Source, unique_object_id},
    },
};
use anyhow::{Context, Result, bail};
use std::{path::PathBuf, sync::Arc};
use suppaftp::tokio::AsyncFtpStream;
use tempfile::NamedTempFile;
use tokio::{
    fs,
    io::{self, AsyncWriteExt},
};

pub(crate) struct FtpUpload {
    pub(crate) object_key: String,
}

pub(crate) async fn upload(
    source: &Source,
    profile: &storage::FtpProfile,
    show_progress: bool,
    rate_limiter: Option<Arc<RateLimiter>>,
) -> Result<FtpUpload> {
    let object_key = remote_object_key(&profile.remote_dir, source);
    let mut client = connect(profile).await?;
    let filename = enter_object_parent(&mut client, &object_key, true).await?;
    if client.size(&filename).await.is_ok() {
        client.quit().await.ok();
        return Ok(FtpUpload { object_key });
    }

    let mut source_file = source.open_file().await?;
    let mut stream = client
        .put_with_stream(&filename)
        .await
        .with_context(|| format!("upload FTP object {object_key}"))?;
    let mut progress = TransferProgress::new("ii send", show_progress, source.size(), 0);
    copy_with_progress_limited(
        &mut source_file,
        &mut stream,
        &mut progress,
        rate_limiter.as_deref(),
    )
    .await
    .with_context(|| format!("upload FTP object {object_key}"))?;
    stream.flush().await.context("flush FTP upload")?;
    progress.finish();
    client
        .finalize_put_stream(stream)
        .await
        .with_context(|| format!("finish FTP upload {object_key}"))?;
    client.quit().await.ok();
    Ok(FtpUpload { object_key })
}

pub(crate) async fn connect(profile: &storage::FtpProfile) -> Result<AsyncFtpStream> {
    storage::validate_ftp_profile(profile)?;
    let url = url::Url::parse(profile.url.trim()).context("parse FTP URL")?;
    let host = url.host_str().context("FTP URL is missing host")?;
    let port = url.port().unwrap_or(21);
    let mut client = AsyncFtpStream::connect((host, port))
        .await
        .with_context(|| format!("connect FTP {host}:{port}"))?;
    client
        .login(&profile.username, &profile.password)
        .await
        .context("authenticate FTP")?;
    Ok(client)
}

fn remote_object_key(remote_dir: &str, source: &Source) -> String {
    match source.content_md5() {
        Some(content_md5) => storage::content_addressed_object_key(remote_dir, content_md5),
        None => storage::normalized_object_key(remote_dir, &unique_object_id(), source.name()),
    }
}

pub(crate) async fn enter_object_parent(
    client: &mut AsyncFtpStream,
    object_key: &str,
    create: bool,
) -> Result<String> {
    let parts = remote_path_parts(object_key)?;
    client.cwd("/").await.context("enter FTP login root")?;
    for part in parts.iter().take(parts.len().saturating_sub(1)) {
        if client.cwd(part).await.is_ok() {
            continue;
        }
        if !create {
            bail!("FTP remote directory is missing {part}");
        }
        client
            .mkdir(part)
            .await
            .with_context(|| format!("create FTP directory {part}"))?;
        client
            .cwd(part)
            .await
            .with_context(|| format!("enter FTP directory {part}"))?;
    }
    Ok(parts
        .last()
        .expect("remote object has a file name")
        .to_string())
}

pub(crate) async fn recv_ftp(
    args: RecvArgs,
    ticket: Ticket,
    out_dir: PathBuf,
    file_target: Option<(PathBuf, FilePlan)>,
    mut trace: RecvTrace,
    show_progress: bool,
) -> Result<()> {
    let ftp = ticket
        .ftp_route()
        .context("ftp ticket missing route")?
        .clone();
    trace.info(format_args!("using FTP object {}", ftp.object_key));
    let checksum_target = file_target.as_ref().map(|(path, _)| path.clone());
    let (profile, save_after_success) = match &ftp.portable {
        Some(portable) => {
            let profile = ftp_profile_from_portable(portable)?;
            let save = portable_ftp_config(&ftp.profile, &profile)?;
            (profile, Some(save))
        }
        None => {
            let selection = if args.json {
                storage::load_ftp_profile_noninteractive(Some(&ftp.profile))?
            } else {
                storage::load_or_prompt_ftp_profile_named(&ftp.profile)?
            };
            let save = selection
                .save_after_success
                .then_some((selection.path.clone(), selection.config.clone()));
            (selection.profile, save)
        }
    };
    let mut client = connect(&profile).await?;
    let bytes_written = match ticket.kind() {
        PayloadKind::File | PayloadKind::Stdin => {
            if args.stdout {
                download_ftp_to_stdout(
                    &mut client,
                    &ftp.object_key,
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
                download_ftp_to_file(
                    &mut client,
                    &ftp.object_key,
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
            download_ftp_tar(
                &mut client,
                &ftp.object_key,
                out_dir,
                ticket.size(),
                show_progress,
                &mut trace,
                args.checksum,
                args.json,
            )
            .await?
        }
    };
    if !args.stdout {
        if let Some(path) = checksum_target {
            super::report_checksum(&args, path).await?;
        }
    }
    if let Some((path, config)) = save_after_success {
        storage::save_config(&path, &config)?;
    }
    trace.step("receive payload");
    trace.info(format_args!("received {} bytes", bytes_written));
    try_delete_ftp(
        &mut client,
        &ftp.object_key,
        ftp.delete_after_recv,
        &mut trace,
    )
    .await;
    client.quit().await.ok();
    trace.finish(bytes_written);
    Ok(())
}

fn ftp_profile_from_portable(portable: &FtpPortableCredentials) -> Result<storage::FtpProfile> {
    let profile = storage::FtpProfile {
        url: portable.url.clone(),
        username: portable.username.clone(),
        password: portable.password.clone(),
        remote_dir: portable.remote_dir.clone(),
    };
    storage::validate_ftp_profile(&profile)?;
    Ok(profile)
}

fn portable_ftp_config(
    profile_name: &str,
    profile: &storage::FtpProfile,
) -> Result<(PathBuf, storage::IiConfig)> {
    let path = storage::default_config_path()?;
    let mut config = storage::load_config(&path)?;
    config
        .storage
        .ftp
        .insert(profile_name.to_string(), profile.clone());
    Ok((path, config))
}

pub(crate) async fn try_delete_ftp_for_ticket(
    ftp: crate::ticket::FtpTicket,
    trace: &mut RecvTrace,
    noninteractive: bool,
) {
    if !ftp.delete_after_recv {
        return;
    }
    let result = async {
        let (profile, save_after_success) = match &ftp.portable {
            Some(portable) => {
                let profile = ftp_profile_from_portable(portable)?;
                let save = portable_ftp_config(&ftp.profile, &profile)?;
                (profile, Some(save))
            }
            None => {
                let selection = if noninteractive {
                    storage::load_ftp_profile_noninteractive(Some(&ftp.profile))?
                } else {
                    storage::load_or_prompt_ftp_profile_named(&ftp.profile)?
                };
                let save = selection
                    .save_after_success
                    .then_some((selection.path.clone(), selection.config.clone()));
                (selection.profile, save)
            }
        };
        let mut client = connect(&profile).await?;
        try_delete_ftp(&mut client, &ftp.object_key, true, trace).await;
        client.quit().await.ok();
        if let Some((path, config)) = save_after_success {
            storage::save_config(&path, &config)?;
        }
        Ok::<(), anyhow::Error>(())
    }
    .await;
    if let Err(err) = result {
        trace.info(format_args!("ftp delete skipped: {err:#}"));
    }
}

pub(crate) async fn download_ftp_to_file(
    client: &mut AsyncFtpStream,
    object_key: &str,
    path: PathBuf,
    resume_from: u64,
    total_size: Option<u64>,
    show_progress: bool,
    trace: &mut RecvTrace,
) -> Result<u64> {
    trace.info(format_args!("download ftp file to {}", path.display()));
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await.ok();
    }
    let filename = enter_object_parent(client, object_key, false).await?;
    let mut append = false;
    if let Ok(offset) = usize::try_from(resume_from)
        && offset > 0
        && client.resume_transfer(offset).await.is_ok()
    {
        append = true;
    }
    let mut response = client
        .retr_as_stream(&filename)
        .await
        .with_context(|| format!("download FTP object {object_key}"))?;
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
    let bytes = copy_with_progress(&mut response, &mut file, &mut progress)
        .await
        .with_context(|| format!("write destination {}", path.display()))?;
    progress.finish();
    file.flush()
        .await
        .with_context(|| format!("flush destination {}", path.display()))?;
    client
        .finalize_retr_stream(response)
        .await
        .with_context(|| format!("finish FTP download {object_key}"))?;
    Ok(bytes)
}

async fn download_ftp_to_stdout(
    client: &mut AsyncFtpStream,
    object_key: &str,
    total_size: Option<u64>,
    show_progress: bool,
    trace: &mut RecvTrace,
) -> Result<u64> {
    trace.info("download ftp file to stdout");
    let filename = enter_object_parent(client, object_key, false).await?;
    let mut response = client
        .retr_as_stream(&filename)
        .await
        .with_context(|| format!("download FTP object {object_key}"))?;
    let mut stdout = io::stdout();
    let mut progress = TransferProgress::new("ii recv", show_progress, total_size, 0);
    let bytes = copy_with_progress(&mut response, &mut stdout, &mut progress)
        .await
        .context("write stdout")?;
    progress.finish();
    stdout.flush().await.ok();
    client
        .finalize_retr_stream(response)
        .await
        .with_context(|| format!("finish FTP download {object_key}"))?;
    Ok(bytes)
}

async fn download_ftp_tar(
    client: &mut AsyncFtpStream,
    object_key: &str,
    out_dir: PathBuf,
    total_size: Option<u64>,
    show_progress: bool,
    trace: &mut RecvTrace,
    checksum: Option<crate::command::ChecksumAlgorithm>,
    json: bool,
) -> Result<u64> {
    trace.info(format_args!("download ftp tar to {}", out_dir.display()));
    fs::create_dir_all(&out_dir)
        .await
        .with_context(|| format!("create output dir {}", out_dir.display()))?;
    let filename = enter_object_parent(client, object_key, false).await?;
    let mut response = client
        .retr_as_stream(&filename)
        .await
        .with_context(|| format!("download FTP object {object_key}"))?;
    let temp = NamedTempFile::new().context("create temp tar")?;
    let temp_path = temp.path().to_path_buf();
    let mut file = fs::File::from_std(temp.reopen().context("reopen temp tar")?);
    let mut progress = TransferProgress::new("ii recv", show_progress, total_size, 0);
    let bytes = copy_with_progress(&mut response, &mut file, &mut progress)
        .await
        .context("buffer ftp tar")?;
    progress.finish();
    file.flush().await.context("flush temp tar")?;
    if let Some(algorithm) = checksum {
        let value =
            crate::transport::source::checksum_path(temp.path().to_path_buf(), algorithm).await?;
        super::report_checksum_value(json, algorithm, &value);
    }
    client
        .finalize_retr_stream(response)
        .await
        .with_context(|| format!("finish FTP download {object_key}"))?;
    let extract_path = out_dir.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let file = std::fs::File::open(&temp_path).context("open tar")?;
        let mut archive = tar::Archive::new(file);
        archive.unpack(&extract_path).context("unpack tar")?;
        Ok(())
    })
    .await
    .context("extract ftp tar task")??;
    Ok(bytes)
}

pub(crate) async fn try_delete_ftp(
    client: &mut AsyncFtpStream,
    object_key: &str,
    delete_after_recv: bool,
    trace: &mut RecvTrace,
) {
    if !delete_after_recv {
        return;
    }
    let result = async {
        let filename = enter_object_parent(client, object_key, false).await?;
        client
            .rm(&filename)
            .await
            .with_context(|| format!("delete FTP object {object_key}"))?;
        Ok::<(), anyhow::Error>(())
    }
    .await;
    match result {
        Ok(()) => trace.info("ftp delete requested after receive"),
        Err(err) => trace.info(format_args!("ftp delete ignored: {err:#}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::source::Source;
    use std::{net::Ipv4Addr, time::Duration};

    #[tokio::test]
    async fn passive_mode_round_trip_deletes_after_receive() {
        let root = tempfile::tempdir().unwrap();
        let port = unused_local_port();
        let ftp_root = root.path().to_path_buf();
        let server = libunftp::ServerBuilder::new(Box::new(move || {
            unftp_sbe_fs::Filesystem::new(ftp_root.clone()).unwrap()
        }))
        .passive_host([127, 0, 0, 1])
        .passive_ports(41000..=41020)
        .build()
        .unwrap();
        let server_task = tokio::spawn(async move {
            let _ = server.listen(format!("127.0.0.1:{port}")).await;
        });
        tokio::time::sleep(Duration::from_millis(100)).await;

        let source_path = root.path().join("source.txt");
        std::fs::write(&source_path, b"ftp payload").unwrap();
        let source = Source::from_file(source_path, None).await.unwrap();
        let profile = storage::FtpProfile {
            url: format!("ftp://127.0.0.1:{port}"),
            username: "user".to_string(),
            password: "pass".to_string(),
            remote_dir: "ii/".to_string(),
        };
        let upload = upload(&source, &profile, false, None).await.unwrap();

        let destination = root.path().join("received.txt");
        let mut client = connect(&profile).await.unwrap();
        let mut trace = RecvTrace::new(false);
        let bytes = download_ftp_to_file(
            &mut client,
            &upload.object_key,
            destination.clone(),
            0,
            source.size(),
            false,
            &mut trace,
        )
        .await
        .unwrap();
        assert_eq!(bytes, 11);
        assert_eq!(std::fs::read(&destination).unwrap(), b"ftp payload");
        try_delete_ftp(&mut client, &upload.object_key, true, &mut trace).await;
        client.quit().await.unwrap();
        assert!(!root.path().join(&upload.object_key).exists());
        server_task.abort();
    }

    fn unused_local_port() -> u16 {
        std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }
}
