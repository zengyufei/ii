use crate::{
    backend::remote_path_parts,
    command::RecvArgs,
    storage,
    ticket::{PayloadKind, SftpPortableAuth, SftpPortableCredentials, Ticket},
    transport::{
        p2p::{FilePlan, RecvTrace},
        progress::{TransferProgress, copy_with_progress},
        source::{Source, unique_object_id},
    },
};
use anyhow::{Context, Result, bail};
use russh::{
    client::{self as ssh_client, Handler as SshClientHandler},
    keys::{HashAlg, PrivateKeyWithHashAlg, decode_secret_key},
};
use russh_sftp::{client::SftpSession, protocol::OpenFlags};
use std::{io::Write, path::PathBuf, sync::Arc};
use tempfile::NamedTempFile;
use tokio::{
    fs,
    io::{self, AsyncSeekExt, AsyncWriteExt},
};

pub(crate) struct SftpUpload {
    pub(crate) object_key: String,
}

struct AcceptAnySftpHost;

impl SshClientHandler for AcceptAnySftpHost {
    type Error = anyhow::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &russh::keys::PublicKey,
    ) -> Result<bool, Self::Error> {
        eprintln!(
            "ii sftp: accepting SSH host key {}",
            server_public_key.fingerprint(HashAlg::Sha256)
        );
        Ok(true)
    }
}

pub(crate) struct SftpConnection {
    _handle: ssh_client::Handle<AcceptAnySftpHost>,
    pub(crate) client: SftpSession,
}

pub(crate) async fn connect(profile: &storage::SftpProfile) -> Result<SftpConnection> {
    storage::validate_sftp_profile(profile)?;
    let config = ssh_client::Config::default();
    let mut handle = ssh_client::connect(
        Arc::new(config),
        (profile.host.as_str(), profile.port),
        AcceptAnySftpHost,
    )
    .await
    .with_context(|| format!("connect SFTP {}:{}", profile.host, profile.port))?;
    let auth = match profile.auth {
        storage::SftpAuth::Password => handle
            .authenticate_password(&profile.username, &profile.password)
            .await
            .context("authenticate SFTP password")?,
        storage::SftpAuth::PrivateKey => {
            let private_key = decode_secret_key(
                &storage::load_sftp_private_key(profile)?,
                profile.private_key_passphrase.as_deref(),
            )
            .context("parse SFTP private key")?;
            let hash_alg = handle
                .best_supported_rsa_hash()
                .await
                .context("negotiate SFTP RSA signature")?
                .flatten();
            handle
                .authenticate_publickey(
                    &profile.username,
                    PrivateKeyWithHashAlg::new(Arc::new(private_key), hash_alg),
                )
                .await
                .context("authenticate SFTP private key")?
        }
    };
    if !auth.success() {
        bail!("SFTP authentication was rejected");
    }
    let channel = handle
        .channel_open_session()
        .await
        .context("open SFTP session channel")?;
    channel
        .request_subsystem(true, "sftp")
        .await
        .context("start SFTP subsystem")?;
    let client = SftpSession::new(channel.into_stream())
        .await
        .context("start SFTP client")?;
    Ok(SftpConnection {
        _handle: handle,
        client,
    })
}

pub(crate) async fn upload(
    source: &Source,
    profile: &storage::SftpProfile,
    show_progress: bool,
) -> Result<SftpUpload> {
    let object_key = remote_object_key(&profile.remote_dir, source);
    let connection = connect(profile).await?;
    ensure_parent_dirs(&connection.client, &object_key).await?;
    if connection.client.try_exists(&object_key).await? {
        connection.client.close().await.ok();
        return Ok(SftpUpload { object_key });
    }
    let mut source_file = source.open_file().await?;
    let mut remote = connection
        .client
        .open_with_flags(
            object_key.clone(),
            OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE,
        )
        .await
        .with_context(|| format!("open SFTP object {object_key}"))?;
    let mut progress = TransferProgress::new("ii send", show_progress, source.size(), 0);
    copy_with_progress(&mut source_file, &mut remote, &mut progress)
        .await
        .with_context(|| format!("upload SFTP object {object_key}"))?;
    remote.flush().await.context("flush SFTP upload")?;
    remote.shutdown().await.context("finish SFTP upload")?;
    progress.finish();
    connection.client.close().await.ok();
    Ok(SftpUpload { object_key })
}

fn remote_object_key(remote_dir: &str, source: &Source) -> String {
    match source.content_md5() {
        Some(content_md5) => storage::content_addressed_object_key(remote_dir, content_md5),
        None => storage::normalized_object_key(remote_dir, &unique_object_id(), source.name()),
    }
}

async fn ensure_parent_dirs(client: &SftpSession, object_key: &str) -> Result<()> {
    let parts = remote_path_parts(object_key)?;
    let mut current = String::new();
    for part in parts.iter().take(parts.len().saturating_sub(1)) {
        if !current.is_empty() {
            current.push('/');
        }
        current.push_str(part);
        if client.try_exists(&current).await? {
            continue;
        }
        client
            .create_dir(&current)
            .await
            .with_context(|| format!("create SFTP directory {current}"))?;
    }
    Ok(())
}

pub(crate) fn portable_credentials(
    profile: &storage::SftpProfile,
) -> Result<SftpPortableCredentials> {
    let auth = match profile.auth {
        storage::SftpAuth::Password => SftpPortableAuth::Password {
            password: profile.password.clone(),
        },
        storage::SftpAuth::PrivateKey => SftpPortableAuth::PrivateKey {
            private_key: storage::load_sftp_private_key(profile)?,
            private_key_passphrase: profile.private_key_passphrase.clone(),
        },
    };
    Ok(SftpPortableCredentials {
        host: profile.host.clone(),
        port: profile.port,
        username: profile.username.clone(),
        remote_dir: profile.remote_dir.clone(),
        auth,
    })
}

struct PortableSftpProfile {
    profile: storage::SftpProfile,
    _private_key: Option<NamedTempFile>,
    private_key_material: Option<String>,
}

pub(crate) async fn recv_sftp(
    args: RecvArgs,
    ticket: Ticket,
    out_dir: PathBuf,
    file_target: Option<(PathBuf, FilePlan)>,
    mut trace: RecvTrace,
    show_progress: bool,
) -> Result<()> {
    let sftp = ticket
        .sftp_route()
        .context("sftp ticket missing route")?
        .clone();
    trace.info(format_args!("using SFTP object {}", sftp.object_key));
    let mut portable_state = None;
    let (profile, save_after_success) = match &sftp.portable {
        Some(portable) => {
            let state = sftp_profile_from_portable(portable)?;
            let profile = state.profile.clone();
            portable_state = Some(state);
            (profile, None)
        }
        None => {
            let selection = storage::load_or_prompt_sftp_profile_named(&sftp.profile)?;
            let save = selection
                .save_after_success
                .then_some((selection.path.clone(), selection.config.clone()));
            (selection.profile, save)
        }
    };
    let connection = connect(&profile).await?;
    let bytes_written = match ticket.kind() {
        PayloadKind::File | PayloadKind::Stdin => {
            if args.stdout {
                download_sftp_to_stdout(
                    &connection.client,
                    &sftp.object_key,
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
                download_sftp_to_file(
                    &connection.client,
                    &sftp.object_key,
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
            download_sftp_tar(
                &connection.client,
                &sftp.object_key,
                out_dir,
                ticket.size(),
                show_progress,
                &mut trace,
            )
            .await?
        }
    };
    if let Some(state) = portable_state.as_ref() {
        let (path, config) = portable_sftp_config(
            &sftp.profile,
            &state.profile,
            state.private_key_material.as_deref(),
        )?;
        storage::save_config(&path, &config)?;
    }
    if let Some((path, config)) = save_after_success {
        storage::save_config(&path, &config)?;
    }
    trace.step("receive payload");
    trace.info(format_args!("received {} bytes", bytes_written));
    try_delete_sftp(
        &connection.client,
        &sftp.object_key,
        sftp.delete_after_recv,
        &mut trace,
    )
    .await;
    connection.client.close().await.ok();
    trace.finish(bytes_written);
    Ok(())
}

fn sftp_profile_from_portable(portable: &SftpPortableCredentials) -> Result<PortableSftpProfile> {
    let (
        auth,
        password,
        private_key_path,
        private_key_passphrase,
        private_key,
        private_key_material,
    ) = match &portable.auth {
        SftpPortableAuth::Password { password } => (
            storage::SftpAuth::Password,
            password.clone(),
            None,
            None,
            None,
            None,
        ),
        SftpPortableAuth::PrivateKey {
            private_key,
            private_key_passphrase,
        } => {
            let mut temp = NamedTempFile::new().context("create temporary SFTP private key")?;
            temp.write_all(private_key.as_bytes())
                .context("write temporary SFTP private key")?;
            let path = temp.path().to_path_buf();
            (
                storage::SftpAuth::PrivateKey,
                String::new(),
                Some(path),
                private_key_passphrase.clone(),
                Some(temp),
                Some(private_key.clone()),
            )
        }
    };
    let profile = storage::SftpProfile {
        host: portable.host.clone(),
        port: portable.port,
        username: portable.username.clone(),
        remote_dir: portable.remote_dir.clone(),
        auth,
        password,
        private_key_path,
        private_key_passphrase,
    };
    storage::validate_sftp_profile(&profile)?;
    Ok(PortableSftpProfile {
        profile,
        _private_key: private_key,
        private_key_material,
    })
}

fn portable_sftp_config(
    profile_name: &str,
    profile: &storage::SftpProfile,
    private_key_material: Option<&str>,
) -> Result<(PathBuf, storage::IiConfig)> {
    let path = storage::default_config_path()?;
    let mut config = storage::load_config(&path)?;
    let mut persisted = profile.clone();
    if let Some(private_key) = private_key_material {
        persisted.private_key_path = Some(storage::save_portable_sftp_private_key(
            profile_name,
            private_key,
        )?);
    }
    config
        .storage
        .sftp
        .insert(profile_name.to_string(), persisted);
    Ok((path, config))
}

pub(crate) async fn try_delete_sftp_for_ticket(
    sftp: crate::ticket::SftpTicket,
    trace: &mut RecvTrace,
) {
    if !sftp.delete_after_recv {
        return;
    }
    let result = async {
        let mut portable_state = None;
        let (profile, save_after_success) = match &sftp.portable {
            Some(portable) => {
                let state = sftp_profile_from_portable(portable)?;
                let profile = state.profile.clone();
                portable_state = Some(state);
                (profile, None)
            }
            None => {
                let selection = storage::load_or_prompt_sftp_profile_named(&sftp.profile)?;
                let save = selection
                    .save_after_success
                    .then_some((selection.path.clone(), selection.config.clone()));
                (selection.profile, save)
            }
        };
        let connection = connect(&profile).await?;
        try_delete_sftp(&connection.client, &sftp.object_key, true, trace).await;
        connection.client.close().await.ok();
        if let Some(state) = portable_state.as_ref() {
            let (path, config) = portable_sftp_config(
                &sftp.profile,
                &state.profile,
                state.private_key_material.as_deref(),
            )?;
            storage::save_config(&path, &config)?;
        }
        if let Some((path, config)) = save_after_success {
            storage::save_config(&path, &config)?;
        }
        Ok::<(), anyhow::Error>(())
    }
    .await;
    if let Err(err) = result {
        trace.info(format_args!("sftp delete skipped: {err:#}"));
    }
}

pub(crate) async fn download_sftp_to_file(
    client: &SftpSession,
    object_key: &str,
    path: PathBuf,
    resume_from: u64,
    total_size: Option<u64>,
    show_progress: bool,
    trace: &mut RecvTrace,
) -> Result<u64> {
    trace.info(format_args!("download sftp file to {}", path.display()));
    remote_path_parts(object_key)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await.ok();
    }
    let mut remote = client
        .open(object_key)
        .await
        .with_context(|| format!("open SFTP object {object_key}"))?;
    let mut append = resume_from > 0;
    if append
        && remote
            .seek(std::io::SeekFrom::Start(resume_from))
            .await
            .is_err()
    {
        append = false;
    }
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
    let bytes = copy_with_progress(&mut remote, &mut file, &mut progress)
        .await
        .with_context(|| format!("write destination {}", path.display()))?;
    progress.finish();
    file.flush()
        .await
        .with_context(|| format!("flush destination {}", path.display()))?;
    Ok(bytes)
}

async fn download_sftp_to_stdout(
    client: &SftpSession,
    object_key: &str,
    total_size: Option<u64>,
    show_progress: bool,
    trace: &mut RecvTrace,
) -> Result<u64> {
    trace.info("download sftp file to stdout");
    remote_path_parts(object_key)?;
    let mut remote = client
        .open(object_key)
        .await
        .with_context(|| format!("open SFTP object {object_key}"))?;
    let mut stdout = io::stdout();
    let mut progress = TransferProgress::new("ii recv", show_progress, total_size, 0);
    let bytes = copy_with_progress(&mut remote, &mut stdout, &mut progress)
        .await
        .context("write stdout")?;
    progress.finish();
    stdout.flush().await.ok();
    Ok(bytes)
}

async fn download_sftp_tar(
    client: &SftpSession,
    object_key: &str,
    out_dir: PathBuf,
    total_size: Option<u64>,
    show_progress: bool,
    trace: &mut RecvTrace,
) -> Result<u64> {
    trace.info(format_args!("download sftp tar to {}", out_dir.display()));
    remote_path_parts(object_key)?;
    fs::create_dir_all(&out_dir)
        .await
        .with_context(|| format!("create output dir {}", out_dir.display()))?;
    let mut remote = client
        .open(object_key)
        .await
        .with_context(|| format!("open SFTP object {object_key}"))?;
    let temp = NamedTempFile::new().context("create temp tar")?;
    let temp_path = temp.path().to_path_buf();
    let mut file = fs::File::from_std(temp.reopen().context("reopen temp tar")?);
    let mut progress = TransferProgress::new("ii recv", show_progress, total_size, 0);
    let bytes = copy_with_progress(&mut remote, &mut file, &mut progress)
        .await
        .context("buffer sftp tar")?;
    progress.finish();
    file.flush().await.context("flush temp tar")?;
    let extract_path = out_dir.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let file = std::fs::File::open(&temp_path).context("open tar")?;
        let mut archive = tar::Archive::new(file);
        archive.unpack(&extract_path).context("unpack tar")?;
        Ok(())
    })
    .await
    .context("extract sftp tar task")??;
    Ok(bytes)
}

pub(crate) async fn try_delete_sftp(
    client: &SftpSession,
    object_key: &str,
    delete_after_recv: bool,
    trace: &mut RecvTrace,
) {
    if !delete_after_recv {
        return;
    }
    let result = async {
        remote_path_parts(object_key)?;
        client
            .remove_file(object_key)
            .await
            .with_context(|| format!("delete SFTP object {object_key}"))?;
        Ok::<(), anyhow::Error>(())
    }
    .await;
    match result {
        Ok(()) => trace.info("sftp delete requested after receive"),
        Err(err) => trace.info(format_args!("sftp delete ignored: {err:#}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use russh::{Channel, ChannelId, server as ssh_server};
    use russh_sftp::{
        protocol::{Attrs, Data, FileAttributes, Handle, Status, StatusCode, Version},
        server::Handler as SftpServerHandler,
    };
    use std::{
        collections::{HashMap, HashSet},
        net::{Ipv4Addr, SocketAddr},
        sync::Arc,
        time::Duration,
    };
    use tokio::sync::Mutex as TokioMutex;

    #[tokio::test]
    async fn password_round_trip_accepts_host_key_and_deletes_after_receive() {
        let state = Arc::new(TestSftpState::default());
        let port = unused_local_port();
        let config = ssh_server::Config {
            keys: vec![
                russh::keys::PrivateKey::random(&mut rand::rng(), russh::keys::Algorithm::Ed25519)
                    .unwrap(),
            ],
            ..Default::default()
        };
        let mut server = TestSftpServer {
            state: Arc::clone(&state),
        };
        let server_task = tokio::spawn(async move {
            let _ = ssh_server::Server::run_on_address(
                &mut server,
                Arc::new(config),
                ("127.0.0.1", port),
            )
            .await;
        });
        tokio::time::sleep(Duration::from_millis(100)).await;

        let root = tempfile::tempdir().unwrap();
        let source_path = root.path().join("source.txt");
        std::fs::write(&source_path, b"sftp payload").unwrap();
        let source = Source::from_file(source_path, None).await.unwrap();
        let profile = storage::SftpProfile {
            host: "127.0.0.1".to_string(),
            port,
            username: "user".to_string(),
            remote_dir: "ii/".to_string(),
            auth: storage::SftpAuth::Password,
            password: "pass".to_string(),
            private_key_path: None,
            private_key_passphrase: None,
        };
        let upload = upload(&source, &profile, false).await.unwrap();

        let destination = root.path().join("received.txt");
        let connection = connect(&profile).await.unwrap();
        let mut trace = RecvTrace::new(false);
        let bytes = download_sftp_to_file(
            &connection.client,
            &upload.object_key,
            destination.clone(),
            0,
            source.size(),
            false,
            &mut trace,
        )
        .await
        .unwrap();
        assert_eq!(bytes, 12);
        assert_eq!(std::fs::read(&destination).unwrap(), b"sftp payload");
        try_delete_sftp(&connection.client, &upload.object_key, true, &mut trace).await;
        connection.client.close().await.unwrap();
        assert!(!state.files.lock().await.contains_key(&upload.object_key));
        server_task.abort();
    }

    fn unused_local_port() -> u16 {
        std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }

    #[derive(Default)]
    struct TestSftpState {
        files: TokioMutex<HashMap<String, Vec<u8>>>,
        dirs: TokioMutex<HashSet<String>>,
    }

    struct TestSftpServer {
        state: Arc<TestSftpState>,
    }

    impl ssh_server::Server for TestSftpServer {
        type Handler = TestSshSession;

        fn new_client(&mut self, _: Option<SocketAddr>) -> Self::Handler {
            TestSshSession {
                state: Arc::clone(&self.state),
                channels: Arc::new(TokioMutex::new(HashMap::new())),
            }
        }
    }

    struct TestSshSession {
        state: Arc<TestSftpState>,
        channels: Arc<TokioMutex<HashMap<ChannelId, Channel<ssh_server::Msg>>>>,
    }

    impl ssh_server::Handler for TestSshSession {
        type Error = anyhow::Error;

        async fn auth_password(
            &mut self,
            _user: &str,
            _password: &str,
        ) -> Result<ssh_server::Auth, Self::Error> {
            Ok(ssh_server::Auth::Accept)
        }

        async fn channel_open_session(
            &mut self,
            channel: Channel<ssh_server::Msg>,
            _session: &mut ssh_server::Session,
        ) -> Result<bool, Self::Error> {
            self.channels.lock().await.insert(channel.id(), channel);
            Ok(true)
        }

        async fn subsystem_request(
            &mut self,
            channel_id: ChannelId,
            name: &str,
            session: &mut ssh_server::Session,
        ) -> Result<(), Self::Error> {
            if name != "sftp" {
                session.channel_failure(channel_id)?;
                return Ok(());
            }
            let channel = self
                .channels
                .lock()
                .await
                .remove(&channel_id)
                .context("missing SFTP test channel")?;
            session.channel_success(channel_id)?;
            russh_sftp::server::run(
                channel.into_stream(),
                TestSftpHandler {
                    state: Arc::clone(&self.state),
                },
            )
            .await;
            Ok(())
        }
    }

    struct TestSftpHandler {
        state: Arc<TestSftpState>,
    }

    impl SftpServerHandler for TestSftpHandler {
        type Error = StatusCode;

        fn unimplemented(&self) -> Self::Error {
            StatusCode::OpUnsupported
        }

        async fn init(
            &mut self,
            _version: u32,
            _extensions: HashMap<String, String>,
        ) -> Result<Version, Self::Error> {
            Ok(Version::new())
        }

        async fn open(
            &mut self,
            id: u32,
            filename: String,
            flags: OpenFlags,
            _attrs: FileAttributes,
        ) -> Result<Handle, Self::Error> {
            let mut files = self.state.files.lock().await;
            if flags.contains(OpenFlags::TRUNCATE) {
                files.insert(filename.clone(), Vec::new());
            } else if flags.contains(OpenFlags::CREATE) {
                files.entry(filename.clone()).or_default();
            } else if !files.contains_key(&filename) {
                return Err(StatusCode::NoSuchFile);
            }
            Ok(Handle {
                id,
                handle: filename,
            })
        }

        async fn close(&mut self, id: u32, _handle: String) -> Result<Status, Self::Error> {
            Ok(test_status(id))
        }

        async fn read(
            &mut self,
            id: u32,
            handle: String,
            offset: u64,
            len: u32,
        ) -> Result<Data, Self::Error> {
            let files = self.state.files.lock().await;
            let bytes = files.get(&handle).ok_or(StatusCode::NoSuchFile)?;
            let start = usize::try_from(offset).map_err(|_| StatusCode::Eof)?;
            if start >= bytes.len() {
                return Err(StatusCode::Eof);
            }
            let end = start.saturating_add(len as usize).min(bytes.len());
            Ok(Data {
                id,
                data: bytes[start..end].to_vec(),
            })
        }

        async fn write(
            &mut self,
            id: u32,
            handle: String,
            offset: u64,
            data: Vec<u8>,
        ) -> Result<Status, Self::Error> {
            let start = usize::try_from(offset).map_err(|_| StatusCode::Failure)?;
            let mut files = self.state.files.lock().await;
            let bytes = files.get_mut(&handle).ok_or(StatusCode::NoSuchFile)?;
            let end = start.saturating_add(data.len());
            if bytes.len() < end {
                bytes.resize(end, 0);
            }
            bytes[start..end].copy_from_slice(&data);
            Ok(test_status(id))
        }

        async fn stat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
            if let Some(bytes) = self.state.files.lock().await.get(&path) {
                let mut attrs = FileAttributes::empty();
                attrs.size = Some(bytes.len() as u64);
                return Ok(Attrs { id, attrs });
            }
            if self.state.dirs.lock().await.contains(&path) {
                return Ok(Attrs {
                    id,
                    attrs: FileAttributes::default(),
                });
            }
            Err(StatusCode::NoSuchFile)
        }

        async fn mkdir(
            &mut self,
            id: u32,
            path: String,
            _attrs: FileAttributes,
        ) -> Result<Status, Self::Error> {
            self.state.dirs.lock().await.insert(path);
            Ok(test_status(id))
        }

        async fn remove(&mut self, id: u32, filename: String) -> Result<Status, Self::Error> {
            self.state.files.lock().await.remove(&filename);
            Ok(test_status(id))
        }
    }

    fn test_status(id: u32) -> Status {
        Status {
            id,
            status_code: StatusCode::Ok,
            error_message: String::new(),
            language_tag: String::new(),
        }
    }
}
