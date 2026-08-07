use super::{
    http::{WebContent, WebShare, serve_web_connection},
    qr::svg as web_qr_svg,
    test_support::*,
};
use crate::transport::source::Source;
use std::{
    net::Ipv4Addr,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::net::TcpListener;

#[tokio::test]
async fn uploads_stream_overwrite_and_reject_invalid_requests() {
    let dir = tempfile::tempdir().unwrap();
    let source_path = dir.path().join("hello.txt");
    let upload_dir = dir.path().join("ii");
    std::fs::write(&source_path, b"web payload").unwrap();
    let sessions = crate::web::upload::sessions();
    let share = Arc::new(WebShare {
        content: WebContent::Download {
            source: Source::from_file(source_path, None).await.unwrap(),
            download_name: "hello.txt".to_string(),
            download_qr_svg: web_qr_svg("http://192.168.1.2:3456/download").unwrap(),
        },
        upload_dir: Some(upload_dir.clone()),
        upload_sessions: Arc::clone(&sessions),
        web_token: None,
        rate_limiter: None,
    });
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        for _ in 0..6 {
            let (stream, _) = listener.accept().await.unwrap();
            let share = Arc::clone(&share);
            tokio::spawn(async move { serve_web_connection(stream, share).await.unwrap() });
        }
    });

    let page = request(address, "/").await;
    for marker in [
        b"type=\"file\" multiple".as_slice(),
        b"fetch('upload/init?name='".as_slice(),
        b"localStorage.getItem(key)".as_slice(),
    ] {
        assert!(page.windows(marker.len()).any(|part| part == marker));
    }

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
        upload_dir: Some(upload_path),
        upload_sessions: crate::web::upload::sessions(),
        web_token: None,
        rate_limiter: None,
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

#[tokio::test]
async fn resumable_upload_commits_after_multiple_chunks() {
    let dir = tempfile::tempdir().unwrap();
    let source_path = dir.path().join("hello.txt");
    let upload_dir = dir.path().join("ii");
    std::fs::write(&source_path, b"web payload").unwrap();
    let sessions = crate::web::upload::sessions();
    let share = Arc::new(WebShare {
        content: WebContent::Download {
            source: Source::from_file(source_path, None).await.unwrap(),
            download_name: "hello.txt".to_string(),
            download_qr_svg: web_qr_svg("http://192.168.1.2:3456/download").unwrap(),
        },
        upload_dir: Some(upload_dir.clone()),
        upload_sessions: Arc::clone(&sessions),
        web_token: None,
        rate_limiter: None,
    });
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        for _ in 0..8 {
            let (stream, _) = listener.accept().await.unwrap();
            let share = Arc::clone(&share);
            tokio::spawn(async move { serve_web_connection(stream, share).await.unwrap() });
        }
    });

    let init = post(address, "/upload/init?name=resume.bin&size=11", b"").await;
    assert!(init.starts_with(b"HTTP/1.1 201 Created"));
    let id = String::from_utf8(response_body(&init).to_vec()).unwrap();
    let upload_path = format!("/upload?name=resume.bin&upload={id}");
    let head = request_with_headers(address, "HEAD", &upload_path, "").await;
    assert_eq!(response_header(&head, "Upload-Offset"), Some("0"));

    let first = patch_request(address, &upload_path, "bytes 0-4/11", b"hello").await;
    assert!(first.starts_with(b"HTTP/1.1 204 No Content"));
    assert_eq!(response_header(&first, "Upload-Offset"), Some("5"));
    let conflict = patch_request(address, &upload_path, "bytes 4-9/11", b" world").await;
    assert!(conflict.starts_with(b"HTTP/1.1 409 Conflict"));
    assert_eq!(response_header(&conflict, "Upload-Offset"), None);
    let second = patch_request(address, &upload_path, "bytes 5-10/11", b" world").await;
    assert!(second.starts_with(b"HTTP/1.1 201 Created"));
    assert_eq!(
        std::fs::read(upload_dir.join("resume.bin")).unwrap(),
        b"hello world"
    );
    let overflow = patch_request(
        address,
        "/upload?name=resume.bin&upload=0123456789abcdef",
        "bytes 0-18446744073709551615/18446744073709551615",
        b"x",
    )
    .await;
    assert!(overflow.starts_with(b"HTTP/1.1 400 Bad Request"));
    let short_init = post(address, "/upload/init?name=short.bin&size=5", b"").await;
    let short_id = String::from_utf8(response_body(&short_init).to_vec()).unwrap();
    let short = raw_request(
        address,
        format!(
            "PATCH /upload?name=short.bin&upload={short_id} HTTP/1.1\r\nHost: test\r\nContent-Length: 5\r\nContent-Range: bytes 0-4/5\r\n\r\nxx"
        )
        .as_bytes(),
    )
    .await;
    assert!(short.starts_with(b"HTTP/1.1 400 Bad Request"));
    assert!(!upload_dir.join("short.bin").exists());
    assert!(sessions.lock().await.is_empty());
    server.await.unwrap();
}

#[tokio::test]
async fn expired_resumable_upload_is_removed() {
    let dir = tempfile::tempdir().unwrap();
    let temp = tempfile::NamedTempFile::new_in(dir.path()).unwrap();
    let temp_path = temp.into_temp_path().keep().unwrap();
    let sessions = crate::web::upload::sessions();
    sessions.lock().await.insert(
        "expired".to_string(),
        crate::web::upload::UploadSession {
            name: "expired.bin".to_string(),
            total: 1,
            offset: 0,
            temp_path: temp_path.clone(),
            last_activity: Instant::now() - Duration::from_secs(60 * 60 + 1),
        },
    );
    crate::web::upload::cleanup_expired(&sessions).await;
    assert!(!temp_path.exists());
    assert!(sessions.lock().await.is_empty());
}

async fn patch_request(
    address: std::net::SocketAddr,
    path: &str,
    range: &str,
    body: &[u8],
) -> Vec<u8> {
    let mut request = format!(
        "PATCH {path} HTTP/1.1\r\nHost: test\r\nContent-Length: {}\r\nContent-Range: {range}\r\n\r\n",
        body.len()
    )
    .into_bytes();
    request.extend_from_slice(body);
    raw_request(address, &request).await
}
