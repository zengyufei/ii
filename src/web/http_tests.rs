use super::{
    http::{
        WebContent, WebServeLifetime, WebShare, bind_lan_web_listener, serve_web_connection,
        serve_web_listener, start_lan_web_server, web_other_hosts, web_root_path, web_upload_dir,
    },
    qr::svg as web_qr_svg,
    test_support::*,
};
use crate::transport::source::Source;
use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    path::Path,
    sync::Arc,
    time::Duration,
};
use tokio::{net::TcpListener, time::timeout};

async fn download_share(
    path: std::path::PathBuf,
    upload_dir: Option<std::path::PathBuf>,
) -> Arc<WebShare> {
    Arc::new(WebShare {
        content: WebContent::Download {
            source: Source::from_file(path, None).await.unwrap(),
            download_name: "hello.txt".to_string(),
            download_qr_svg: web_qr_svg("http://192.168.1.2:3456/download").unwrap(),
        },
        upload_dir,
        upload_sessions: crate::web::upload::sessions(),
        web_token: None,
        rate_limiter: None,
    })
}

async fn assert_listener_is_running(server: &mut tokio::task::JoinHandle<anyhow::Result<()>>) {
    assert!(timeout(Duration::from_millis(100), server).await.is_err());
}

#[tokio::test]
async fn one_download_listener_ignores_page_and_upload_then_stops() {
    let dir = tempfile::tempdir().unwrap();
    let source_path = dir.path().join("hello.txt");
    let upload_dir = dir.path().join("ii");
    std::fs::write(&source_path, b"web payload").unwrap();
    let share = download_share(source_path, Some(upload_dir.clone())).await;
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let address = listener.local_addr().unwrap();
    let mut server = tokio::spawn(serve_web_listener(
        listener,
        share,
        WebServeLifetime::OneSuccessfulDownload,
    ));

    assert!(request(address, "/").await.starts_with(b"HTTP/1.1 200 OK"));
    assert_listener_is_running(&mut server).await;

    assert!(
        upload_request(address, "notes.txt", b"upload")
            .await
            .starts_with(b"HTTP/1.1 201 Created")
    );
    assert_eq!(
        std::fs::read(upload_dir.join("notes.txt")).unwrap(),
        b"upload"
    );
    assert_listener_is_running(&mut server).await;

    assert!(
        request(address, "/download")
            .await
            .starts_with(b"HTTP/1.1 200 OK")
    );
    timeout(Duration::from_secs(1), &mut server)
        .await
        .expect("listener should stop after the completed download")
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn one_download_listener_ignores_failed_download() {
    let dir = tempfile::tempdir().unwrap();
    let source_path = dir.path().join("hello.txt");
    std::fs::write(&source_path, b"web payload").unwrap();
    let share = download_share(source_path.clone(), None).await;
    std::fs::remove_file(source_path).unwrap();
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let address = listener.local_addr().unwrap();
    let mut server = tokio::spawn(serve_web_listener(
        listener,
        share,
        WebServeLifetime::OneSuccessfulDownload,
    ));

    assert!(
        request(address, "/download")
            .await
            .starts_with(b"HTTP/1.1 200 OK")
    );
    assert_listener_is_running(&mut server).await;
    assert!(request(address, "/").await.starts_with(b"HTTP/1.1 200 OK"));
    assert_listener_is_running(&mut server).await;
    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn keep_alive_web_listener_serves_multiple_downloads() {
    let dir = tempfile::tempdir().unwrap();
    let source_path = dir.path().join("hello.txt");
    std::fs::write(&source_path, b"web payload").unwrap();
    let share = download_share(source_path, None).await;
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let address = listener.local_addr().unwrap();
    let mut server = tokio::spawn(serve_web_listener(
        listener,
        share,
        WebServeLifetime::UntilCtrlC,
    ));

    for _ in 0..2 {
        assert!(
            request(address, "/download")
                .await
                .starts_with(b"HTTP/1.1 200 OK")
        );
        assert_listener_is_running(&mut server).await;
    }
    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn directory_once_stops_only_after_a_full_file_get() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("shared");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("movie.mp4"), b"0123456789").unwrap();
    let share = Arc::new(WebShare {
        content: WebContent::Directory {
            root: std::fs::canonicalize(&root).unwrap(),
        },
        upload_dir: None,
        upload_sessions: crate::web::upload::sessions(),
        web_token: None,
        rate_limiter: None,
    });
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let address = listener.local_addr().unwrap();
    let mut server = tokio::spawn(serve_web_listener(
        listener,
        share,
        WebServeLifetime::OneSuccessfulDownload,
    ));

    assert!(request(address, "/").await.starts_with(b"HTTP/1.1 200 OK"));
    assert_listener_is_running(&mut server).await;
    assert!(
        request_with_headers(address, "HEAD", "/movie.mp4", "")
            .await
            .starts_with(b"HTTP/1.1 200 OK")
    );
    assert_listener_is_running(&mut server).await;
    assert!(
        request_with_headers(address, "GET", "/movie.mp4", "Range: bytes=2-5\r\n")
            .await
            .starts_with(b"HTTP/1.1 206 Partial Content")
    );
    assert_listener_is_running(&mut server).await;
    assert!(
        request(address, "/missing.bin")
            .await
            .starts_with(b"HTTP/1.1 404 Not Found")
    );
    assert_listener_is_running(&mut server).await;
    let full = request(address, "/movie.mp4").await;
    assert!(full.starts_with(b"HTTP/1.1 200 OK"));
    assert_eq!(response_body(&full), b"0123456789");
    timeout(Duration::from_secs(1), &mut server)
        .await
        .expect("directory listener should stop after full file GET")
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn download_share_serves_page_download_and_token_routes() {
    let dir = tempfile::tempdir().unwrap();
    let source_path = dir.path().join("hello.txt");
    let upload_dir = dir.path().join("uploads");
    let token = "A1b2C3d4E5f6G7h8";
    std::fs::write(&source_path, b"web payload").unwrap();
    let share = Arc::new(WebShare {
        content: WebContent::Download {
            source: Source::from_file(source_path, None).await.unwrap(),
            download_name: "hello.txt".to_string(),
            download_qr_svg: web_qr_svg(&format!("http://192.168.1.2:3456/{token}/download"))
                .unwrap(),
        },
        upload_dir: None,
        upload_sessions: crate::web::upload::sessions(),
        web_token: Some(token.to_string()),
        rate_limiter: None,
    });
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        for _ in 0..5 {
            let (stream, _) = listener.accept().await.unwrap();
            let share = Arc::clone(&share);
            tokio::spawn(async move { serve_web_connection(stream, share).await.unwrap() });
        }
    });

    let page = request(address, &format!("/{token}/")).await;
    assert!(page.starts_with(b"HTTP/1.1 200 OK"));
    for marker in [
        b"hello.txt".as_slice(),
        b"<svg".as_slice(),
        b"name=\"viewport\"".as_slice(),
        b"width:min(82vw,17.5rem)".as_slice(),
        b"href=\"download\"".as_slice(),
    ] {
        assert!(page.windows(marker.len()).any(|part| part == marker));
    }
    for marker in [
        b"type=\"file\" multiple".as_slice(),
        b"fetch('upload?name='".as_slice(),
    ] {
        assert!(!page.windows(marker.len()).any(|part| part == marker));
    }

    let download = request(address, &format!("/{token}/download")).await;
    assert!(download.starts_with(b"HTTP/1.1 200 OK"));
    assert!(download.ends_with(b"web payload"));
    assert_eq!(response_header(&download, "Accept-Ranges"), None);
    assert!(
        upload_request_at(
            address,
            &format!("/{token}/upload?name=notes.txt"),
            b"blocked upload",
        )
        .await
        .starts_with(b"HTTP/1.1 404 Not Found")
    );
    assert!(!upload_dir.exists());

    for path in ["/", "/download"] {
        assert!(
            request(address, path)
                .await
                .starts_with(b"HTTP/1.1 404 Not Found")
        );
    }
    server.await.unwrap();
}

#[test]
fn lan_url_helpers_preserve_default_token_and_upload_paths() {
    let base = tempfile::tempdir().unwrap();
    let absolute = base.path().join("absolute");
    assert_eq!(web_root_path(None), "/");
    assert_eq!(
        web_root_path(Some("A1b2C3d4E5f6G7h8")),
        "/A1b2C3d4E5f6G7h8/"
    );
    assert_eq!(web_upload_dir(base.path(), None), base.path().join("ii"));
    assert_eq!(
        web_upload_dir(base.path(), Some(Path::new("relative"))),
        base.path().join("relative")
    );
    assert_eq!(web_upload_dir(base.path(), Some(&absolute)), absolute);

    let primary = Ipv4Addr::new(192, 168, 1, 8);
    assert_eq!(
        web_other_hosts(
            primary,
            vec![
                Ipv4Addr::UNSPECIFIED,
                Ipv4Addr::LOCALHOST,
                primary,
                Ipv4Addr::new(172, 17, 0, 1),
                Ipv4Addr::new(10, 0, 0, 2),
                Ipv4Addr::new(172, 17, 0, 1),
            ],
        ),
        vec![Ipv4Addr::new(10, 0, 0, 2), Ipv4Addr::new(172, 17, 0, 1)]
    );
}

#[tokio::test]
async fn lan_listener_uses_random_or_requested_port() {
    let random = bind_lan_web_listener(None, None, "test").await.unwrap();
    assert_ne!(random.local_addr().unwrap().port(), 0);
    drop(random);

    let reservation = TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)).await.unwrap();
    let port = reservation.local_addr().unwrap().port();
    drop(reservation);
    let requested = bind_lan_web_listener(Some(port), None, "test")
        .await
        .unwrap();
    assert_eq!(requested.local_addr().unwrap().port(), port);
    drop(requested);

    let occupied = TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)).await.unwrap();
    assert!(
        bind_lan_web_listener(Some(occupied.local_addr().unwrap().port()), None, "test")
            .await
            .is_err()
    );
}

#[tokio::test]
async fn explicit_ipv6_listener_uses_a_bracketed_url() {
    let lan = start_lan_web_server(None, Some(IpAddr::V6(Ipv6Addr::LOCALHOST)), None, "test").await;
    let Ok(lan) = lan else {
        return;
    };
    assert!(lan.listener.local_addr().unwrap().is_ipv6());
    assert!(lan.url.starts_with("http://[::1]:"));
}

#[tokio::test]
async fn lan_server_url_uses_requested_port_and_token_path() {
    let reservation = TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)).await.unwrap();
    let port = reservation.local_addr().unwrap().port();
    drop(reservation);

    let lan = start_lan_web_server(Some(port), None, Some("A1b2C3d4E5f6G7h8"), "test")
        .await
        .unwrap();
    assert_eq!(lan.listener.local_addr().unwrap().port(), port);
    assert!(lan.url.ends_with(&format!(":{port}/A1b2C3d4E5f6G7h8/")));
}
