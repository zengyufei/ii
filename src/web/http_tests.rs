use super::{
    http::{
        WebContent, WebShare, bind_lan_web_listener, serve_web_connection, start_lan_web_server,
        web_other_hosts, web_root_path, web_upload_dir,
    },
    qr::svg as web_qr_svg,
    test_support::*,
};
use crate::transport::source::Source;
use std::{net::Ipv4Addr, path::Path, sync::Arc};
use tokio::net::TcpListener;

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
        web_token: Some(token.to_string()),
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
    let random = bind_lan_web_listener(None, "test").await.unwrap();
    assert_ne!(random.local_addr().unwrap().port(), 0);
    drop(random);

    let reservation = TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)).await.unwrap();
    let port = reservation.local_addr().unwrap().port();
    drop(reservation);
    let requested = bind_lan_web_listener(Some(port), "test").await.unwrap();
    assert_eq!(requested.local_addr().unwrap().port(), port);
    drop(requested);

    let occupied = TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)).await.unwrap();
    assert!(
        bind_lan_web_listener(Some(occupied.local_addr().unwrap().port()), "test")
            .await
            .is_err()
    );
}

#[tokio::test]
async fn lan_server_url_uses_requested_port_and_token_path() {
    let reservation = TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0)).await.unwrap();
    let port = reservation.local_addr().unwrap().port();
    drop(reservation);

    let lan = start_lan_web_server(Some(port), Some("A1b2C3d4E5f6G7h8"), "test")
        .await
        .unwrap();
    assert_eq!(lan.listener.local_addr().unwrap().port(), port);
    assert!(lan.url.ends_with(&format!(":{port}/A1b2C3d4E5f6G7h8/")));
}
