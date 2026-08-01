use super::{
    test_support::*,
    webrtc::{
        WEBRTC_MAX_PENDING_SIGNALS, WEBRTC_PEER_TTL, WebRtcRelayError, WebRtcServer,
        serve_connection,
    },
};
use std::{net::Ipv4Addr, sync::Arc, time::Instant};
use tokio::net::TcpListener;

#[tokio::test]
async fn webrtc_serves_token_page_and_relays_signals() {
    let token = "A1b2C3d4E5f6G7h8";
    let state = Arc::new(WebRtcServer::new());
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        for _ in 0..7 {
            let (stream, _) = listener.accept().await.unwrap();
            let state = Arc::clone(&state);
            let token = token.to_string();
            tokio::spawn(
                async move { serve_connection(stream, state, Some(token)).await.unwrap() },
            );
        }
    });
    let page = request(address, &format!("/{token}/")).await;
    for marker in [
        b"RTCPeerConnection".as_slice(),
        b"iceServers: []".as_slice(),
        b"type=\"file\" multiple".as_slice(),
        b"TEXT_CHUNK_SIZE = 8 * 1024".as_slice(),
    ] {
        assert!(page.windows(marker.len()).any(|part| part == marker));
    }
    assert!(
        request(address, "/")
            .await
            .starts_with(b"HTTP/1.1 404 Not Found")
    );
    assert!(
        post(address, &format!("/{token}/join"), b"")
            .await
            .ends_with(b"\r\n\r\n1")
    );
    assert!(
        post(address, &format!("/{token}/join"), b"")
            .await
            .ends_with(b"\r\n\r\n2")
    );
    let signal_body = br#"{"type":"offer","description":{"type":"offer"}}"#;
    assert!(
        post(
            address,
            &format!("/{token}/signal?from=1&to=2"),
            signal_body
        )
        .await
        .starts_with(b"HTTP/1.1 204 No Content")
    );
    let delivered = request(address, &format!("/{token}/signal?id=2")).await;
    assert_eq!(response_header(&delivered, "X-II-From"), Some("1"));
    assert!(delivered.ends_with(signal_body));
    assert!(
        request(address, &format!("/{token}/download"))
            .await
            .starts_with(b"HTTP/1.1 404 Not Found")
    );
    server.await.unwrap();
}

#[test]
fn webrtc_members_expire_and_pending_signals_are_bounded() {
    let server = WebRtcServer::new();
    let first = server.join().unwrap();
    let second = server.join().unwrap();
    for _ in 0..WEBRTC_MAX_PENDING_SIGNALS {
        server.relay(first, second, b"{}".to_vec()).unwrap();
    }
    assert!(matches!(
        server.relay(first, second, b"{}".to_vec()),
        Err(WebRtcRelayError::QueueFull)
    ));
    server
        .state
        .lock()
        .unwrap()
        .peers
        .get_mut(&first)
        .unwrap()
        .last_seen = Instant::now() - WEBRTC_PEER_TTL;
    assert_eq!(server.peers(first), None);
}
