use std::sync::Arc;

use anyhow::{Context, Result, bail};
use digest_auth::{AuthContext, HttpMethod, WwwAuthenticateHeader};
use quick_xml::{Reader, events::Event};
use reqwest::{Method, RequestBuilder};
use tokio::sync::Mutex;
use url::Url;

#[derive(Clone, Debug)]
pub enum Auth {
    Basic(String, String),
    Digest(String, String),
}

#[derive(Clone, Debug)]
pub struct Client {
    agent: reqwest::Client,
    host: String,
    auth: Auth,
    digest: Arc<Mutex<Option<WwwAuthenticateHeader>>>,
}

#[derive(Debug)]
pub struct PropfindResponse {
    status: u16,
    body: Vec<u8>,
}

impl PropfindResponse {
    pub fn status(&self) -> u16 {
        self.status
    }

    pub fn is_multistatus(&self) -> Result<bool> {
        if !(200..300).contains(&self.status) {
            return Ok(false);
        }
        let mut reader = Reader::from_reader(self.body.as_slice());
        let mut buffer = Vec::new();
        let mut root = None;
        let mut depth = 0usize;
        let mut finished = false;
        loop {
            match reader.read_event_into(&mut buffer)? {
                Event::Start(element) if root.is_none() => {
                    root = Some(element.local_name().as_ref() == b"multistatus");
                    depth = 1;
                }
                Event::Empty(element) if root.is_none() => {
                    root = Some(element.local_name().as_ref() == b"multistatus");
                    finished = true;
                }
                Event::Start(_) | Event::Empty(_) if finished => {
                    bail!("malformed WebDAV XML: multiple root elements");
                }
                Event::Start(_) => {
                    depth += 1;
                }
                Event::End(_) => {
                    depth = depth.checked_sub(1).context("malformed WebDAV XML")?;
                    finished = depth == 0;
                }
                Event::Text(text)
                    if (root.is_none() || finished)
                        && text.iter().any(|byte| !byte.is_ascii_whitespace()) =>
                {
                    bail!("malformed WebDAV XML: text outside root element");
                }
                Event::CData(_) if root.is_none() || finished => {
                    bail!("malformed WebDAV XML: CDATA outside root element");
                }
                Event::Eof if depth == 0 => return Ok(root == Some(true)),
                Event::Eof => bail!("malformed WebDAV XML: unclosed element"),
                _ => {}
            }
            buffer.clear();
        }
    }
}

impl Client {
    pub fn new(host: String, auth: Auth) -> Result<Self> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let url = Url::parse(&host).context("parse WebDAV URL")?;
        if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
            bail!("WebDAV URL must be an absolute HTTP or HTTPS URL");
        }
        Ok(Self {
            agent: reqwest::Client::new(),
            host,
            auth,
            digest: Arc::new(Mutex::new(None)),
        })
    }

    pub async fn start_request(&self, method: Method, path: &str) -> Result<RequestBuilder> {
        let url = Url::parse(&format!(
            "{}/{}",
            self.host.trim_end_matches('/'),
            path.trim_start_matches('/')
        ))
        .context("build WebDAV request URL")?;
        let builder = self.agent.request(method.clone(), url.as_str());
        self.apply_authentication(builder, &method, &url).await
    }

    pub async fn mkcol(&self, path: &str) -> Result<u16> {
        let response = self
            .start_request(Method::from_bytes(b"MKCOL")?, path)
            .await?
            .send()
            .await?;
        Ok(response.status().as_u16())
    }

    pub async fn propfind(&self, path: &str) -> Result<PropfindResponse> {
        let response = self
            .start_request(Method::from_bytes(b"PROPFIND")?, path)
            .await?
            .header("depth", "0")
            .header("content-type", "text/xml; charset=\"utf-8\"")
            .body("<?xml version=\"1.0\" encoding=\"utf-8\" ?><D:propfind xmlns:D=\"DAV:\"><D:allprop/></D:propfind>")
            .send()
            .await?;
        let status = response.status().as_u16();
        let body = if (200..300).contains(&status) {
            response.bytes().await?.to_vec()
        } else {
            Vec::new()
        };
        Ok(PropfindResponse { status, body })
    }

    pub async fn delete(&self, path: &str) -> Result<u16> {
        let response = self
            .start_request(Method::DELETE, path)
            .await?
            .send()
            .await?;
        Ok(response.status().as_u16())
    }

    async fn apply_authentication(
        &self,
        mut builder: RequestBuilder,
        method: &Method,
        url: &Url,
    ) -> Result<RequestBuilder> {
        match &self.auth {
            Auth::Basic(username, password) => {
                builder = builder.basic_auth(username, Some(password));
            }
            Auth::Digest(username, password) => {
                self.initialize_digest(method, url).await?;
                let mut context = AuthContext::new(username, password, url.path());
                context.method = HttpMethod::from(method.to_string());
                let mut state = self.digest.lock().await;
                let state = state
                    .as_mut()
                    .context("missing WebDAV digest auth context")?;
                builder =
                    builder.header("Authorization", state.respond(&context)?.to_header_string());
            }
        }
        Ok(builder)
    }

    async fn initialize_digest(&self, method: &Method, url: &Url) -> Result<()> {
        if self.digest.lock().await.is_some() {
            return Ok(());
        }
        let response = self
            .agent
            .request(method.clone(), url.as_str())
            .send()
            .await?;
        if response.status().as_u16() != 401 {
            bail!(
                "WebDAV Digest auth probe expected 401, got {}",
                response.status()
            );
        }
        let header = response
            .headers()
            .get("www-authenticate")
            .context("WebDAV Digest auth probe response lacks WWW-Authenticate")?
            .to_str()?;
        let mut state = self.digest.lock().await;
        *state = Some(digest_auth::parse(header)?);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use tokio::{
        io::{AsyncReadExt as _, AsyncWriteExt as _},
        net::TcpListener,
    };

    use super::*;

    async fn read_request(stream: &mut tokio::net::TcpStream) -> String {
        let mut bytes = Vec::new();
        let mut chunk = [0_u8; 1024];
        loop {
            let read = stream.read(&mut chunk).await.unwrap();
            assert_ne!(
                read, 0,
                "HTTP client disconnected before completing request"
            );
            bytes.extend_from_slice(&chunk[..read]);
            if bytes.windows(4).any(|window| window == b"\r\n\r\n") {
                return String::from_utf8(bytes).unwrap();
            }
        }
    }

    #[tokio::test]
    async fn basic_auth_is_sent_on_webdav_request() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let request = read_request(&mut stream).await;
            stream
                .write_all(
                    b"HTTP/1.1 201 Created\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .await
                .unwrap();
            request
        });
        let client = Client::new(
            format!("http://{address}/dav"),
            Auth::Basic("user".to_owned(), "password".to_owned()),
        )
        .unwrap();
        assert_eq!(client.mkcol("parent").await.unwrap(), 201);
        let request = server.await.unwrap();
        assert!(request.starts_with("MKCOL /dav/parent HTTP/1.1"));
        assert!(request.contains("authorization: Basic dXNlcjpwYXNzd29yZA"));
    }

    #[tokio::test]
    async fn digest_auth_probes_then_sends_authorization() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut first, _) = listener.accept().await.unwrap();
            let probe = read_request(&mut first).await;
            first
                .write_all(b"HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Digest realm=\"example.com\", qop=\"auth\", nonce=\"nonce\", opaque=\"opaque\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .await
                .unwrap();
            let (mut second, _) = listener.accept().await.unwrap();
            let request = read_request(&mut second).await;
            second
                .write_all(
                    b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .await
                .unwrap();
            (probe, request)
        });
        let client = Client::new(
            format!("http://{address}/dav"),
            Auth::Digest("user".to_owned(), "password".to_owned()),
        )
        .unwrap();
        assert_eq!(client.delete("object").await.unwrap(), 204);
        let (probe, request) = server.await.unwrap();
        assert!(probe.starts_with("DELETE /dav/object HTTP/1.1"));
        assert!(!probe.to_ascii_lowercase().contains("authorization:"));
        assert!(request.starts_with("DELETE /dav/object HTTP/1.1"));
        assert!(request.contains("authorization: Digest"));
        assert!(request.contains("username=\"user\""));
        assert!(request.contains("uri=\"/dav/object\""));
    }

    #[tokio::test]
    async fn propfind_uses_depth_zero_and_validates_multistatus() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let request = read_request(&mut stream).await;
            let body = b"<D:multistatus xmlns:D=\"DAV:\"/>";
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 207 Multi-Status\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            stream.write_all(body).await.unwrap();
            request
        });
        let client = Client::new(
            format!("http://{address}/dav"),
            Auth::Basic("user".to_owned(), "password".to_owned()),
        )
        .unwrap();
        let response = client.propfind("object").await.unwrap();
        assert_eq!(response.status(), 207);
        assert!(response.is_multistatus().unwrap());
        let request = server.await.unwrap();
        assert!(request.starts_with("PROPFIND /dav/object HTTP/1.1"));
        assert!(request.contains("depth: 0"));
        assert!(request.contains("<D:allprop/>"));
    }

    #[test]
    fn propfind_rejects_malformed_or_non_webdav_xml() {
        let malformed = PropfindResponse {
            status: 207,
            body: b"<D:multistatus xmlns:D=\"DAV:\">".to_vec(),
        };
        assert!(malformed.is_multistatus().is_err());

        let non_webdav = PropfindResponse {
            status: 207,
            body: b"<ok/>".to_vec(),
        };
        assert!(!non_webdav.is_multistatus().unwrap());

        let multiple_roots = PropfindResponse {
            status: 207,
            body: b"<D:multistatus xmlns:D=\"DAV:\"/><unexpected/>".to_vec(),
        };
        assert!(multiple_roots.is_multistatus().is_err());

        let root_external_cdata = PropfindResponse {
            status: 207,
            body: b"<D:multistatus xmlns:D=\"DAV:\"/><![CDATA[unexpected]]>".to_vec(),
        };
        assert!(root_external_cdata.is_multistatus().is_err());
    }

    #[test]
    fn client_rejects_non_http_or_relative_urls() {
        let relative = Client::new(
            "/dav".to_owned(),
            Auth::Basic("user".to_owned(), "pass".to_owned()),
        )
        .unwrap_err();
        assert!(relative.to_string().contains("parse WebDAV URL"));

        let unsupported_scheme = Client::new(
            "ftp://dav.example.test".to_owned(),
            Auth::Basic("user".to_owned(), "pass".to_owned()),
        )
        .unwrap_err();
        assert!(
            unsupported_scheme
                .to_string()
                .contains("absolute HTTP or HTTPS")
        );
    }

    #[test]
    fn unsuccessful_propfind_is_not_a_multistatus_response() {
        let response = PropfindResponse {
            status: 404,
            body: b"not found".to_vec(),
        };

        assert!(!response.is_multistatus().unwrap());
    }
}
