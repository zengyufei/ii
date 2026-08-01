use super::{
    http::{WebContent, WebShare, serve_web_connection},
    qr::svg as web_qr_svg,
    test_support::*,
};
use crate::transport::source::Source;
use std::{net::Ipv4Addr, sync::Arc};
use tokio::net::TcpListener;

#[tokio::test]
async fn uploads_stream_overwrite_and_reject_invalid_requests() {
    let dir = tempfile::tempdir().unwrap();
    let source_path = dir.path().join("hello.txt");
    let upload_dir = dir.path().join("ii");
    std::fs::write(&source_path, b"web payload").unwrap();
    let share = Arc::new(WebShare {
        content: WebContent::Download {
            source: Source::from_file(source_path, None).await.unwrap(),
            download_name: "hello.txt".to_string(),
            download_qr_svg: web_qr_svg("http://192.168.1.2:3456/download").unwrap(),
        },
        upload_dir: upload_dir.clone(),
        web_token: None,
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

    for body in [b"first upload".as_slice(), b"second upload".as_slice()] {
        let response = upload_request(address, "notes.txt", body).await;
        assert!(response.starts_with(b"HTTP/1.1 201 Created"));
        assert!(response.ends_with(b"saved: notes.txt"));
    }
    assert_eq!(
        std::fs::read(upload_dir.join("notes.txt")).unwrap(),
        b"second upload"
    );
    assert!(!upload_dir.join("notes (1).txt").exists());

    assert!(
        upload_request(address, "..%2Fescape.txt", b"invalid")
            .await
            .starts_with(b"HTTP/1.1 400 Bad Request")
    );
    assert!(
        raw_request(
            address,
            b"POST /upload?name=missing.txt HTTP/1.1\r\nHost: test\r\n\r\n"
        )
        .await
        .starts_with(b"HTTP/1.1 411 Length Required")
    );
    assert!(
        raw_request(
            address,
            b"POST /upload?name=short.txt HTTP/1.1\r\nHost: test\r\nContent-Length: 8\r\n\r\nshort"
        )
        .await
        .starts_with(b"HTTP/1.1 400 Bad Request")
    );
    assert!(!upload_dir.join("short.txt").exists());
    server.await.unwrap();
}

#[tokio::test]
async fn upload_reports_directory_creation_failure() {
    let dir = tempfile::tempdir().unwrap();
    let source_path = dir.path().join("hello.txt");
    let upload_path = dir.path().join("not-a-directory");
    std::fs::write(&source_path, b"web payload").unwrap();
    std::fs::write(&upload_path, b"blocked").unwrap();
    let share = Arc::new(WebShare {
        content: WebContent::Download {
            source: Source::from_file(source_path, None).await.unwrap(),
            download_name: "hello.txt".to_string(),
            download_qr_svg: web_qr_svg("http://192.168.1.2:3456/download").unwrap(),
        },
        upload_dir: upload_path,
        web_token: None,
    });
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        serve_web_connection(stream, share).await.unwrap();
    });
    assert!(
        upload_request(address, "failed.txt", b"upload")
            .await
            .starts_with(b"HTTP/1.1 500 Internal Server Error")
    );
    server.await.unwrap();
}
