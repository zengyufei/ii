use std::{net::SocketAddr, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
pub(crate) async fn request(address: SocketAddr, path: &str) -> Vec<u8> {
    request_with_headers(address, "GET", path, "").await
}

pub(crate) async fn request_with_headers(
    address: SocketAddr,
    method: &str,
    path: &str,
    headers: &str,
) -> Vec<u8> {
    raw_request(
        address,
        format!("{method} {path} HTTP/1.1\r\nHost: test\r\n{headers}\r\n").as_bytes(),
    )
    .await
}

pub(crate) async fn upload_request(address: SocketAddr, name: &str, body: &[u8]) -> Vec<u8> {
    upload_request_at(address, &format!("/upload?name={name}"), body).await
}

pub(crate) async fn upload_request_at(address: SocketAddr, path: &str, body: &[u8]) -> Vec<u8> {
    let mut request = format!(
        "POST {path} HTTP/1.1\r\nHost: test\r\nContent-Length: {}\r\n\r\n",
        body.len()
    )
    .into_bytes();
    request.extend_from_slice(body);
    raw_request(address, &request).await
}

pub(crate) async fn post(address: SocketAddr, path: &str, body: &[u8]) -> Vec<u8> {
    let mut request = format!(
        "POST {path} HTTP/1.1\r\nHost: test\r\nContent-Length: {}\r\n\r\n",
        body.len()
    )
    .into_bytes();
    request.extend_from_slice(body);
    raw_request(address, &request).await
}

pub(crate) async fn raw_request(address: SocketAddr, request: &[u8]) -> Vec<u8> {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut stream = TcpStream::connect(address).await.unwrap();
        stream.write_all(request).await.unwrap();
        stream.shutdown().await.unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.unwrap();
        response
    })
    .await
    .unwrap()
}

pub(crate) fn response_header<'a>(response: &'a [u8], name: &str) -> Option<&'a str> {
    let header_end = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")?;
    std::str::from_utf8(&response[..header_end])
        .ok()?
        .split("\r\n")
        .skip(1)
        .find_map(|line| {
            let (header_name, value) = line.split_once(':')?;
            header_name
                .eq_ignore_ascii_case(name)
                .then_some(value.trim())
        })
}

pub(crate) fn response_body(response: &[u8]) -> &[u8] {
    let header_end = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("response headers");
    &response[header_end + 4..]
}
