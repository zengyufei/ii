use crate::{command::FtpArgs, transport::progress::RateLimiter, web::http::lan_ipv4_hosts};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use futures_util::{Stream, TryStreamExt};
use libunftp::{
    ServerBuilder,
    options::{ActivePassiveMode, FtpsRequired, Shutdown},
};
use std::{
    fmt::Debug,
    io,
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
};
use tokio_util::io::{ReaderStream, StreamReader};
use unftp_core::{
    auth::{AuthenticationError, Authenticator, Credentials, DefaultUser, Principal},
    storage::{
        Error as StorageError, ErrorKind, Fileinfo, Result as StorageResult, StorageBackend,
    },
};
use unftp_sbe_fs::{Filesystem, Meta};

#[derive(Debug, Clone, Copy)]
struct Access {
    upload: bool,
    download: bool,
    delete: bool,
    rename: bool,
    mkdir: bool,
}

#[derive(Debug)]
struct FtpStorage {
    inner: Filesystem,
    access: Access,
    rate: Option<Arc<RateLimiter>>,
}

impl FtpStorage {
    fn new(inner: Filesystem, access: Access, rate: Option<Arc<RateLimiter>>) -> Self {
        Self {
            inner,
            access,
            rate,
        }
    }
}

fn denied(operation: &str) -> StorageError {
    StorageError::new(
        ErrorKind::PermissionDenied,
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("FTP {operation} disabled"),
        ),
    )
}

type LimitedStream = Pin<Box<dyn Stream<Item = Result<bytes::Bytes, io::Error>> + Send + Sync>>;

fn limited_reader<R>(reader: R, rate: Arc<RateLimiter>) -> StreamReader<LimitedStream, bytes::Bytes>
where
    R: tokio::io::AsyncRead + Send + Sync + Unpin + 'static,
{
    let stream = ReaderStream::new(reader).and_then(move |bytes| {
        let rate = Arc::clone(&rate);
        async move {
            rate.wait(bytes.len()).await;
            Ok(bytes)
        }
    });
    StreamReader::new(Box::pin(stream))
}

#[async_trait]
impl StorageBackend<DefaultUser> for FtpStorage {
    type Metadata = Meta;

    fn enter(&mut self, user: &DefaultUser) -> io::Result<()> {
        self.inner.enter(user)
    }

    fn supported_features(&self) -> u32 {
        <Filesystem as StorageBackend<DefaultUser>>::supported_features(&self.inner)
    }

    async fn metadata<P: AsRef<Path> + Send + Debug>(
        &self,
        user: &DefaultUser,
        path: P,
    ) -> StorageResult<Self::Metadata> {
        self.inner.metadata(user, path).await
    }

    async fn list<P: AsRef<Path> + Send + Debug>(
        &self,
        user: &DefaultUser,
        path: P,
    ) -> StorageResult<Vec<Fileinfo<PathBuf, Self::Metadata>>> {
        self.inner.list(user, path).await
    }

    async fn get<P: AsRef<Path> + Send + Debug>(
        &self,
        user: &DefaultUser,
        path: P,
        start_pos: u64,
    ) -> StorageResult<Box<dyn tokio::io::AsyncRead + Send + Sync + Unpin>> {
        if !self.access.download {
            return Err(denied("download"));
        }
        let reader = self.inner.get(user, path, start_pos).await?;
        match self.rate.as_ref() {
            Some(rate) => Ok(Box::new(limited_reader(reader, Arc::clone(rate)))),
            None => Ok(reader),
        }
    }

    async fn put<P, R>(
        &self,
        user: &DefaultUser,
        input: R,
        path: P,
        start_pos: u64,
    ) -> StorageResult<u64>
    where
        P: AsRef<Path> + Send + Debug,
        R: tokio::io::AsyncRead + Send + Sync + Unpin + 'static,
    {
        if !self.access.upload {
            return Err(denied("upload"));
        }
        match self.rate.as_ref() {
            Some(rate) => {
                self.inner
                    .put(
                        user,
                        limited_reader(input, Arc::clone(rate)),
                        path,
                        start_pos,
                    )
                    .await
            }
            None => self.inner.put(user, input, path, start_pos).await,
        }
    }

    async fn del<P: AsRef<Path> + Send + Debug>(
        &self,
        user: &DefaultUser,
        path: P,
    ) -> StorageResult<()> {
        if !self.access.delete {
            return Err(denied("delete"));
        }
        self.inner.del(user, path).await
    }

    async fn mkd<P: AsRef<Path> + Send + Debug>(
        &self,
        user: &DefaultUser,
        path: P,
    ) -> StorageResult<()> {
        if !self.access.mkdir {
            return Err(denied("mkdir"));
        }
        self.inner.mkd(user, path).await
    }

    async fn rename<P: AsRef<Path> + Send + Debug>(
        &self,
        user: &DefaultUser,
        from: P,
        to: P,
    ) -> StorageResult<()> {
        if !self.access.rename {
            return Err(denied("rename"));
        }
        self.inner.rename(user, from, to).await
    }

    async fn rmd<P: AsRef<Path> + Send + Debug>(
        &self,
        user: &DefaultUser,
        path: P,
    ) -> StorageResult<()> {
        if !self.access.delete {
            return Err(denied("delete"));
        }
        self.inner.rmd(user, path).await
    }

    async fn cwd<P: AsRef<Path> + Send + Debug>(
        &self,
        user: &DefaultUser,
        path: P,
    ) -> StorageResult<()> {
        self.inner.cwd(user, path).await
    }
}

#[derive(Debug)]
struct FixedAuthenticator {
    username: String,
    password: String,
}

#[async_trait]
impl Authenticator for FixedAuthenticator {
    async fn authenticate(
        &self,
        username: &str,
        credentials: &Credentials,
    ) -> Result<Principal, AuthenticationError> {
        if username != self.username {
            return Err(AuthenticationError::BadUser);
        }
        if credentials.password.as_deref() != Some(self.password.as_str()) {
            return Err(AuthenticationError::BadPassword);
        }
        Ok(Principal {
            username: username.to_string(),
        })
    }
}

pub(super) async fn run(args: FtpArgs) -> Result<()> {
    let start = std::env::current_dir().context("read current directory for FTP service")?;
    let root = match args.dir {
        Some(path) if path.is_absolute() => path,
        Some(path) => start.join(path),
        None => start,
    };
    let metadata = tokio::fs::metadata(&root)
        .await
        .with_context(|| format!("read FTP directory {}", root.display()))?;
    if !metadata.is_dir() {
        bail!("FTP share path is not a directory: {}", root.display());
    }
    Filesystem::new(root.clone()).context("open FTP share directory")?;

    print_addresses(args.bind, args.port, args.tls);
    println!("shared directory: {}", root.display());
    println!("max connections: {}", args.max_connections);
    println!(
        "mode: {}",
        if args.passive_ports.is_some() {
            "active+passive"
        } else {
            "active"
        }
    );
    if args.tls {
        println!(
            "TLS: {}{}",
            if args.implicit_tls {
                "implicit"
            } else {
                "explicit"
            },
            if args.cert.is_some() {
                " (configured certificate)"
            } else {
                " (temporary self-signed certificate)"
            }
        );
    }
    println!("press Ctrl+C to stop FTP server");

    let access = Access {
        upload: args.upload,
        download: args.download,
        delete: args.delete,
        rename: args.rename,
        mkdir: args.mkdir,
    };
    let rate = args.rate.map(RateLimiter::new).map(Arc::new);
    let factory_root = root.clone();
    let factory_rate = rate.clone();
    let factory = Box::new(move || {
        let filesystem =
            Filesystem::new(factory_root.clone()).expect("validated FTP share directory");
        FtpStorage::new(filesystem, access, factory_rate.clone())
    });

    let mut builder = match (args.username, args.password) {
        (Some(username), Some(password)) => ServerBuilder::with_authenticator(
            factory,
            Arc::new(FixedAuthenticator { username, password }),
        ),
        (None, None) => ServerBuilder::new(factory),
        _ => unreachable!("FTP credentials are validated by the CLI"),
    }
    .active_passive_mode(if args.passive_ports.is_some() {
        ActivePassiveMode::ActiveAndPassive
    } else {
        ActivePassiveMode::ActiveOnly
    })
    .max_connections(args.max_connections)
    .shutdown_indicator(async {
        let _ = tokio::signal::ctrl_c().await;
        Shutdown::new()
    });

    if args.tls {
        let config =
            crate::relay::tls_server_config(None, args.cert.as_deref(), args.key.as_deref())
                .context("build FTP TLS certificate configuration")?;
        builder = if args.implicit_tls {
            builder.ftps_implicit_manual(Arc::new(config))
        } else {
            builder.ftps_manual(Arc::new(config))
        }
        .ftps_required(FtpsRequired::All, FtpsRequired::All);
    }

    if let Some(passive_ports) = args.passive_ports {
        builder = builder.passive_ports(passive_ports);
        if let Some(passive_host) = args.passive_host.as_deref() {
            builder = builder.passive_host(passive_host);
        }
    }
    let server = builder.build().context("build FTP server")?;
    server
        .listen(SocketAddr::new(args.bind, args.port).to_string())
        .await
        .context("run FTP server")
}

fn print_addresses(bind: IpAddr, port: u16, tls: bool) {
    let scheme = if tls { "ftps" } else { "ftp" };
    println!(
        "ii ftp listener: {scheme}://{}",
        SocketAddr::new(bind, port)
    );
    match bind {
        IpAddr::V4(ip) if ip.is_unspecified() => {
            let (primary, other) = lan_ipv4_hosts();
            println!("ii ftp: {scheme}://{primary}:{port}/");
            println!();
            println!("other:");
            for host in other {
                println!("{scheme}://{host}:{port}/");
            }
        }
        IpAddr::V6(ip) if ip.is_unspecified() => {
            println!(
                "ii ftp: {scheme}://[{}]:{port}/",
                crate::web::http::local_web_host_v6()
            );
        }
        ip => println!("ii ftp: {scheme}://{ip}:{port}/"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustls::{
        DigitallySignedStruct, SignatureScheme,
        client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
        pki_types::{CertificateDer, ServerName, UnixTime},
    };
    use std::{net::Ipv4Addr, sync::Arc, time::Duration};
    use tokio::{
        io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader},
        net::TcpStream,
        task::JoinHandle,
    };
    use tokio_rustls::TlsConnector;

    #[derive(Debug)]
    struct AcceptAnyCertificate;

    impl ServerCertVerifier for AcceptAnyCertificate {
        fn verify_server_cert(
            &self,
            _end_entity: &CertificateDer,
            _intermediates: &[CertificateDer],
            _server_name: &ServerName,
            _ocsp_response: &[u8],
            _now: UnixTime,
        ) -> Result<ServerCertVerified, rustls::Error> {
            Ok(ServerCertVerified::assertion())
        }

        fn verify_tls12_signature(
            &self,
            _message: &[u8],
            _cert: &CertificateDer<'_>,
            _dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            Ok(HandshakeSignatureValid::assertion())
        }

        fn verify_tls13_signature(
            &self,
            _message: &[u8],
            _cert: &CertificateDer<'_>,
            _dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            Ok(HandshakeSignatureValid::assertion())
        }

        fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
            rustls::crypto::ring::default_provider()
                .signature_verification_algorithms
                .supported_schemes()
        }
    }

    fn unused_local_port() -> u16 {
        std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }

    fn args(dir: PathBuf, port: u16) -> FtpArgs {
        FtpArgs {
            dir: Some(dir),
            port,
            bind: IpAddr::V4(Ipv4Addr::LOCALHOST),
            username: None,
            password: None,
            rate: None,
            max_connections: 100,
            upload: true,
            download: true,
            delete: true,
            rename: true,
            mkdir: true,
            tls: false,
            implicit_tls: false,
            cert: None,
            key: None,
            passive_host: None,
            passive_ports: None,
        }
    }

    async fn start(args: FtpArgs) -> JoinHandle<()> {
        let port = args.port;
        let task = tokio::spawn(async move {
            let _ = run(args).await;
        });
        for _ in 0..50 {
            if let Ok(stream) = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).await {
                drop(stream);
                tokio::time::sleep(Duration::from_millis(20)).await;
                return task;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        task.abort();
        panic!("FTP server did not start on port {port}");
    }

    async fn active_client(port: u16) -> suppaftp::tokio::AsyncFtpStream {
        let mut client = suppaftp::tokio::AsyncFtpStream::connect((Ipv4Addr::LOCALHOST, port))
            .await
            .unwrap()
            .active_mode(Duration::from_secs(2));
        client.login("anonymous", "").await.unwrap();
        client
    }

    fn tls_connector() -> TlsConnector {
        crate::install_crypto_provider();
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .unwrap()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AcceptAnyCertificate))
            .with_no_client_auth();
        TlsConnector::from(Arc::new(config))
    }

    async fn read_reply<S>(reader: &mut BufReader<S>) -> String
    where
        S: AsyncRead + Unpin,
    {
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        line
    }

    #[tokio::test]
    async fn active_mode_upload_download_and_resume_work() {
        let root = tempfile::tempdir().unwrap();
        let port = unused_local_port();
        let task = start(args(root.path().to_path_buf(), port)).await;

        let source_path = root.path().join("source.bin");
        tokio::fs::write(&source_path, b"abcdef").await.unwrap();
        let mut client = active_client(port).await;
        let mut source = tokio::fs::File::open(&source_path).await.unwrap();
        client.put_file("uploaded.bin", &mut source).await.unwrap();

        let append_path = root.path().join("append.bin");
        tokio::fs::write(&append_path, b"XYZ").await.unwrap();
        client.resume_transfer(3).await.unwrap();
        let mut append = tokio::fs::File::open(&append_path).await.unwrap();
        client.put_file("uploaded.bin", &mut append).await.unwrap();

        let mut stream = client.retr_as_stream("uploaded.bin").await.unwrap();
        let mut received = Vec::new();
        stream.read_to_end(&mut received).await.unwrap();
        client.finalize_retr_stream(stream).await.unwrap();
        client.quit().await.unwrap();
        assert_eq!(received, b"abcXYZ");
        task.abort();
    }

    #[tokio::test]
    async fn active_only_rejects_pasv_and_epsv() {
        let root = tempfile::tempdir().unwrap();
        let port = unused_local_port();
        let task = start(args(root.path().to_path_buf(), port)).await;
        let stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port))
            .await
            .unwrap();
        let mut reader = BufReader::new(stream);
        assert!(read_reply(&mut reader).await.starts_with("220"));
        reader
            .get_mut()
            .write_all(b"USER anonymous\r\n")
            .await
            .unwrap();
        assert!(read_reply(&mut reader).await.starts_with("331"));
        reader.get_mut().write_all(b"PASS \r\n").await.unwrap();
        assert!(read_reply(&mut reader).await.starts_with("230"));
        reader.get_mut().write_all(b"PASV\r\n").await.unwrap();
        assert!(read_reply(&mut reader).await.starts_with("502"));
        reader.get_mut().write_all(b"EPSV\r\n").await.unwrap();
        assert!(read_reply(&mut reader).await.starts_with("502"));
        task.abort();
    }

    #[tokio::test]
    async fn passive_ports_enable_passive_and_keep_active() {
        let root = tempfile::tempdir().unwrap();
        let port = unused_local_port();
        let passive_port = unused_local_port();
        let mut ftp_args = args(root.path().to_path_buf(), port);
        ftp_args.passive_host = Some("127.0.0.1".to_string());
        ftp_args.passive_ports = Some(passive_port..=passive_port);
        let task = start(ftp_args).await;

        let source_path = root.path().join("source.bin");
        tokio::fs::write(&source_path, b"passive").await.unwrap();
        let mut passive = suppaftp::tokio::AsyncFtpStream::connect((Ipv4Addr::LOCALHOST, port))
            .await
            .unwrap();
        passive.login("anonymous", "").await.unwrap();
        let mut source = tokio::fs::File::open(&source_path).await.unwrap();
        passive.put_file("passive.bin", &mut source).await.unwrap();
        passive.quit().await.unwrap();

        let mut active = active_client(port).await;
        let mut stream = active.retr_as_stream("passive.bin").await.unwrap();
        let mut received = Vec::new();
        stream.read_to_end(&mut received).await.unwrap();
        active.finalize_retr_stream(stream).await.unwrap();
        active.quit().await.unwrap();
        assert_eq!(received, b"passive");
        task.abort();
    }

    #[tokio::test]
    async fn credentials_permissions_and_control_limit_are_enforced() {
        let root = tempfile::tempdir().unwrap();
        tokio::fs::write(root.path().join("existing.bin"), b"content")
            .await
            .unwrap();
        tokio::fs::create_dir(root.path().join("existing-dir"))
            .await
            .unwrap();
        let port = unused_local_port();
        let mut ftp_args = args(root.path().to_path_buf(), port);
        ftp_args.username = Some("alice".to_string());
        ftp_args.password = Some("secret".to_string());
        ftp_args.max_connections = 1;
        ftp_args.upload = false;
        ftp_args.download = false;
        ftp_args.delete = false;
        ftp_args.rename = false;
        ftp_args.mkdir = false;
        let task = start(ftp_args).await;

        let first = TcpStream::connect((Ipv4Addr::LOCALHOST, port))
            .await
            .unwrap();
        let mut first = BufReader::new(first);
        assert!(read_reply(&mut first).await.starts_with("220"));
        let second = TcpStream::connect((Ipv4Addr::LOCALHOST, port))
            .await
            .unwrap();
        let mut second = BufReader::new(second);
        assert!(read_reply(&mut second).await.starts_with("421"));
        drop(first);
        drop(second);
        tokio::time::sleep(Duration::from_millis(20)).await;

        let mut wrong = suppaftp::tokio::AsyncFtpStream::connect((Ipv4Addr::LOCALHOST, port))
            .await
            .unwrap()
            .active_mode(Duration::from_secs(2));
        assert!(wrong.login("alice", "wrong").await.is_err());
        drop(wrong);
        tokio::time::sleep(Duration::from_millis(20)).await;

        let mut client = suppaftp::tokio::AsyncFtpStream::connect((Ipv4Addr::LOCALHOST, port))
            .await
            .unwrap()
            .active_mode(Duration::from_secs(2));
        client.login("alice", "secret").await.unwrap();
        let source_path = root.path().join("source.bin");
        tokio::fs::write(&source_path, b"blocked").await.unwrap();
        let mut source = tokio::fs::File::open(&source_path).await.unwrap();
        assert!(client.put_file("blocked.bin", &mut source).await.is_err());
        let mut stream = client.retr_as_stream("existing.bin").await.unwrap();
        let mut received = Vec::new();
        let _ = stream.read_to_end(&mut received).await;
        assert!(client.finalize_retr_stream(stream).await.is_err());
        assert!(client.mkdir("new-dir").await.is_err());
        assert!(client.rm("existing.bin").await.is_err());
        assert!(client.rmdir("existing-dir").await.is_err());
        assert!(client.rename("existing.bin", "renamed.bin").await.is_err());
        task.abort();
    }

    #[tokio::test]
    async fn explicit_ftps_requires_auth_tls_and_private_data_channel() {
        let root = tempfile::tempdir().unwrap();
        tokio::fs::write(root.path().join("secure.bin"), b"secure payload")
            .await
            .unwrap();
        let port = unused_local_port();
        let mut ftp_args = args(root.path().to_path_buf(), port);
        ftp_args.tls = true;
        let task = start(ftp_args).await;

        let stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port))
            .await
            .unwrap();
        let mut reader = BufReader::new(stream);
        assert!(read_reply(&mut reader).await.starts_with("220"));
        reader.get_mut().write_all(b"AUTH TLS\r\n").await.unwrap();
        assert!(read_reply(&mut reader).await.starts_with("234"));

        let stream = tls_connector()
            .connect(
                ServerName::try_from("localhost").unwrap().to_owned(),
                reader.into_inner(),
            )
            .await
            .unwrap();
        let mut reader = BufReader::new(stream);
        reader.get_mut().write_all(b"PBSZ 0\r\n").await.unwrap();
        assert!(read_reply(&mut reader).await.starts_with("200"));
        reader.get_mut().write_all(b"PROT C\r\n").await.unwrap();
        assert!(read_reply(&mut reader).await.starts_with("534"));
        reader.get_mut().write_all(b"PROT P\r\n").await.unwrap();
        assert!(read_reply(&mut reader).await.starts_with("200"));
        reader
            .get_mut()
            .write_all(b"USER anonymous\r\n")
            .await
            .unwrap();
        assert!(read_reply(&mut reader).await.starts_with("331"));
        reader.get_mut().write_all(b"PASS \r\n").await.unwrap();
        assert!(read_reply(&mut reader).await.starts_with("230"));

        let data_listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let data_port = data_listener.local_addr().unwrap().port();
        let [high, low] = data_port.to_be_bytes();
        reader
            .get_mut()
            .write_all(format!("PORT 127,0,0,1,{high},{low}\r\n").as_bytes())
            .await
            .unwrap();
        assert!(read_reply(&mut reader).await.starts_with("200"));
        reader
            .get_mut()
            .write_all(b"RETR secure.bin\r\n")
            .await
            .unwrap();
        assert!(read_reply(&mut reader).await.starts_with("150"));
        let (data_stream, _) = data_listener.accept().await.unwrap();
        let mut data_stream = tls_connector()
            .connect(
                ServerName::try_from("localhost").unwrap().to_owned(),
                data_stream,
            )
            .await
            .unwrap();
        let mut contents = Vec::new();
        data_stream.read_to_end(&mut contents).await.unwrap();
        assert_eq!(contents, b"secure payload");
        assert!(read_reply(&mut reader).await.starts_with("226"));
        task.abort();
    }

    #[tokio::test]
    async fn implicit_ftps_handshakes_before_the_ftp_greeting() {
        let root = tempfile::tempdir().unwrap();
        let port = unused_local_port();
        let mut ftp_args = args(root.path().to_path_buf(), port);
        ftp_args.tls = true;
        ftp_args.implicit_tls = true;
        let task = start(ftp_args).await;

        let stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port))
            .await
            .unwrap();
        let stream = tls_connector()
            .connect(
                ServerName::try_from("localhost").unwrap().to_owned(),
                stream,
            )
            .await
            .unwrap();
        let mut reader = BufReader::new(stream);
        assert!(read_reply(&mut reader).await.starts_with("220"));
        reader.get_mut().write_all(b"AUTH TLS\r\n").await.unwrap();
        assert!(read_reply(&mut reader).await.starts_with("503"));
        reader.get_mut().write_all(b"PROT C\r\n").await.unwrap();
        assert!(read_reply(&mut reader).await.starts_with("534"));
        reader
            .get_mut()
            .write_all(b"USER anonymous\r\n")
            .await
            .unwrap();
        assert!(read_reply(&mut reader).await.starts_with("331"));
        reader.get_mut().write_all(b"PASS \r\n").await.unwrap();
        assert!(read_reply(&mut reader).await.starts_with("230"));
        task.abort();
    }
}
