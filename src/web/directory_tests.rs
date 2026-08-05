use super::{
    directory::directory_root,
    http::{WebContent, WebShare, serve_web_connection},
    test_support::*,
};
use std::{net::Ipv4Addr, path::Path, sync::Arc};
use tokio::{fs, net::TcpListener};

#[tokio::test]
async fn directory_lists_children_rejects_traversal_and_uploads() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("shared");
    let nested = root.join("nested");
    let upload_dir = temp.path().join("uploads");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(root.join("top file.txt"), b"top").unwrap();
    std::fs::write(nested.join("child.txt"), b"child payload").unwrap();
    let share = Arc::new(WebShare {
        content: WebContent::Directory {
            root: fs::canonicalize(&root).await.unwrap(),
        },
        upload_dir: Some(upload_dir.clone()),
        web_token: None,
        rate_limiter: None,
    });
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        for _ in 0..7 {
            let (stream, _) = listener.accept().await.unwrap();
            let share = Arc::clone(&share);
            tokio::spawn(async move { serve_web_connection(stream, share).await.unwrap() });
        }
    });

    let root_page = request(address, "/").await;
    for marker in [
        b"Index of /".as_slice(),
        b"nested/".as_slice(),
        b"top file.txt".as_slice(),
        b"type=\"file\" multiple".as_slice(),
    ] {
        assert!(root_page.windows(marker.len()).any(|part| part == marker));
    }
    let redirect = request(address, "/nested").await;
    assert!(redirect.starts_with(b"HTTP/1.1 302 Found"));
    assert!(
        redirect
            .windows(b"Location: /nested/".len())
            .any(|part| part == b"Location: /nested/")
    );
    let child = request(address, "/nested/child.txt").await;
    assert!(child.starts_with(b"HTTP/1.1 200 OK"));
    assert!(child.ends_with(b"child payload"));
    for path in [
        "/%2e%2e/secret.txt",
        "/nested%2fchild.txt",
        "/nested%5cchild.txt",
    ] {
        assert!(
            request(address, path)
                .await
                .starts_with(b"HTTP/1.1 404 Not Found")
        );
    }
    assert!(
        upload_request(address, "notes.bin", b"replacement upload")
            .await
            .starts_with(b"HTTP/1.1 201 Created")
    );
    assert_eq!(
        std::fs::read(upload_dir.join("notes.bin")).unwrap(),
        b"replacement upload"
    );
    server.await.unwrap();
}

#[tokio::test]
async fn directory_files_support_ranges_and_token_scoping() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("shared");
    let upload_dir = temp.path().join("uploads");
    let token = "A1b2C3d4E5f6G7h8";
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("range.mp4"), b"0123456789").unwrap();
    std::fs::write(root.join("empty.bin"), b"").unwrap();
    let share = Arc::new(WebShare {
        content: WebContent::Directory {
            root: fs::canonicalize(&root).await.unwrap(),
        },
        upload_dir: None,
        web_token: Some(token.to_string()),
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
    let path = format!("/{token}/range.mp4");
    let full = request(address, &path).await;
    assert_eq!(response_header(&full, "Content-Type"), Some("video/mp4"));
    assert_eq!(response_header(&full, "Accept-Ranges"), Some("bytes"));
    assert_eq!(response_body(&full), b"0123456789");
    let range = request_with_headers(address, "GET", &path, "Range: bytes=2-5\r\n").await;
    assert!(range.starts_with(b"HTTP/1.1 206 Partial Content"));
    assert_eq!(
        response_header(&range, "Content-Range"),
        Some("bytes 2-5/10")
    );
    assert_eq!(response_body(&range), b"2345");
    let page = request(address, &format!("/{token}/")).await;
    assert!(
        !page
            .windows(b"type=\"file\" multiple".len())
            .any(|part| part == b"type=\"file\" multiple")
    );
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
    let invalid = request_with_headers(address, "GET", &path, "Range: bytes=10-10\r\n").await;
    assert!(invalid.starts_with(b"HTTP/1.1 416 Range Not Satisfiable"));
    assert_eq!(
        response_header(&invalid, "Content-Range"),
        Some("bytes */10")
    );
    for path in ["/", &format!("/{token}"), "/wrong-token/"] {
        assert!(
            request(address, path)
                .await
                .starts_with(b"HTTP/1.1 404 Not Found")
        );
    }
    server.await.unwrap();
}

#[tokio::test]
async fn directory_root_requires_an_existing_directory() {
    let temp = tempfile::tempdir().unwrap();
    let directory = temp.path().join("directory");
    let file = temp.path().join("file.txt");
    std::fs::create_dir(&directory).unwrap();
    std::fs::write(&file, b"file").unwrap();
    assert_eq!(
        directory_root(temp.path(), None).await.unwrap(),
        fs::canonicalize(temp.path()).await.unwrap()
    );
    assert_eq!(
        directory_root(temp.path(), Some(Path::new("directory")))
            .await
            .unwrap(),
        fs::canonicalize(&directory).await.unwrap()
    );
    assert!(directory_root(temp.path(), Some(&file)).await.is_err());
    assert!(
        directory_root(temp.path(), Some(&temp.path().join("missing")))
            .await
            .is_err()
    );
}
