use crate::{
    storage::{self, AzureAuth, AzureProfile},
    transport::{
        progress::{RateLimiter, TransferProgress},
        source::{Source, unique_object_id},
    },
};
use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use futures_util::TryStreamExt;
use hmac::{Hmac, Mac};
use percent_encoding::percent_decode_str;
use sha2::Sha256;
use std::{
    collections::BTreeMap,
    io::SeekFrom,
    path::Path,
    sync::{Arc, Mutex},
};
use time::{Duration, OffsetDateTime, format_description::FormatItem, macros::format_description};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio_util::io::ReaderStream;
use url::Url;

const SERVICE_VERSION: &str = "2020-12-06";
const MAX_BLOCKS: u64 = 50_000;
const MIN_BLOCK_BYTES: u64 = 8 * 1024 * 1024;
const MAX_BLOCK_BYTES: u64 = 4_000 * 1024 * 1024;
const MIB: u64 = 1024 * 1024;
const HTTP_DATE: &[FormatItem<'static>] = format_description!(
    "[weekday repr:short], [day padding:zero] [month repr:short] [year] [hour padding:zero]:[minute]:[second] GMT"
);
const SAS_TIME: &[FormatItem<'static>] =
    format_description!("[year]-[month]-[day]T[hour]:[minute]:[second]Z");

type HmacSha256 = Hmac<Sha256>;

pub(crate) struct AzureUpload {
    pub(crate) download_url: String,
    pub(crate) delete_url: Option<String>,
    pub(crate) object_key: String,
}

pub(crate) async fn upload(
    source: &Source,
    profile: &AzureProfile,
    delete_after_recv: bool,
    show_progress: bool,
    rate_limiter: Option<Arc<RateLimiter>>,
) -> Result<AzureUpload> {
    let client = AzureClient::new(profile)?;
    let object_key = match source.content_md5() {
        Some(content_md5) => storage::content_addressed_object_key(&profile.prefix, content_md5),
        None => storage::normalized_object_key(&profile.prefix, &unique_object_id(), source.name()),
    };
    if !client.object_exists(&object_key).await? {
        client
            .upload_source(source, &object_key, show_progress, rate_limiter)
            .await?;
    }
    let download_url = client.signed_object_url(&object_key, "r")?;
    let delete_url = delete_after_recv
        .then(|| client.signed_object_url(&object_key, "d"))
        .transpose()?;
    Ok(AzureUpload {
        download_url,
        delete_url,
        object_key,
    })
}

struct AzureClient {
    profile: AzureProfile,
    endpoint: Url,
    http: reqwest::Client,
}

impl AzureClient {
    fn new(profile: &AzureProfile) -> Result<Self> {
        storage::validate_azure_profile(profile)?;
        let endpoint = match profile.endpoint.as_deref() {
            Some(endpoint) => endpoint.to_string(),
            None => format!(
                "https://{}.blob.core.windows.net",
                profile.account_name.trim()
            ),
        };
        let endpoint = Url::parse(&endpoint).context("parse Azure endpoint")?;
        Ok(Self {
            profile: profile.clone(),
            endpoint,
            http: reqwest::Client::new(),
        })
    }

    async fn object_exists(&self, key: &str) -> Result<bool> {
        match self
            .request(
                reqwest::Method::HEAD,
                self.object_url(key)?,
                None,
                None,
                None,
                &[],
            )
            .await?
        {
            200..=299 => Ok(true),
            404 => Ok(false),
            status => bail!("Azure object check failed with status {status}"),
        }
    }

    async fn upload_source(
        &self,
        source: &Source,
        key: &str,
        show_progress: bool,
        rate_limiter: Option<Arc<RateLimiter>>,
    ) -> Result<()> {
        if source.size == 0 {
            return self.put_empty_blob(key).await;
        }
        let block_size = block_size(source.size)?;
        let source_path = source.local_path();
        let progress = Arc::new(Mutex::new(TransferProgress::new(
            "ii send",
            show_progress,
            source.size(),
            0,
        )));
        let mut block_ids = Vec::new();
        let mut remaining = source.size;
        let mut offset = 0_u64;
        let mut index = 0_u64;
        while remaining > 0 {
            let length = remaining.min(block_size);
            let block_id = block_id(index);
            self.put_block(
                key,
                &block_id,
                &source_path,
                offset,
                length,
                Arc::clone(&progress),
                rate_limiter.clone(),
            )
            .await?;
            block_ids.push(block_id);
            remaining -= length;
            offset += length;
            index += 1;
        }
        self.put_block_list(key, &block_ids).await?;
        if let Ok(mut progress) = progress.lock() {
            progress.finish();
        }
        Ok(())
    }

    async fn put_empty_blob(&self, key: &str) -> Result<()> {
        let status = self
            .request(
                reqwest::Method::PUT,
                self.object_url(key)?,
                Some(reqwest::Body::from(Vec::new())),
                Some(0),
                Some("application/octet-stream"),
                &[("x-ms-blob-type", "BlockBlob")],
            )
            .await?;
        ensure_success(status, "create empty Azure blob")
    }

    async fn put_block(
        &self,
        key: &str,
        block_id: &str,
        source_path: &Path,
        offset: u64,
        length: u64,
        progress: Arc<Mutex<TransferProgress>>,
        rate_limiter: Option<Arc<RateLimiter>>,
    ) -> Result<()> {
        let mut file = tokio::fs::File::open(source_path)
            .await
            .with_context(|| format!("open source file {}", source_path.display()))?;
        file.seek(SeekFrom::Start(offset))
            .await
            .context("seek Azure block source")?;
        let reader = file.take(length);
        let stream = ReaderStream::new(reader).and_then(move |bytes| {
            let progress = Arc::clone(&progress);
            let rate_limiter = rate_limiter.clone();
            async move {
                if let Some(rate_limiter) = rate_limiter {
                    rate_limiter.wait(bytes.len()).await;
                }
                if let Ok(mut progress) = progress.lock() {
                    progress.advance(bytes.len() as u64);
                }
                Ok::<_, std::io::Error>(bytes)
            }
        });
        let mut url = self.object_url(key)?;
        url.query_pairs_mut()
            .append_pair("comp", "block")
            .append_pair("blockid", block_id);
        let status = self
            .request(
                reqwest::Method::PUT,
                url,
                Some(reqwest::Body::wrap_stream(stream)),
                Some(length),
                Some("application/octet-stream"),
                &[],
            )
            .await?;
        ensure_success(status, "upload Azure block")
    }

    async fn put_block_list(&self, key: &str, block_ids: &[String]) -> Result<()> {
        let mut body = String::from("<?xml version=\"1.0\" encoding=\"utf-8\"?><BlockList>");
        for block_id in block_ids {
            body.push_str("<Latest>");
            body.push_str(block_id);
            body.push_str("</Latest>");
        }
        body.push_str("</BlockList>");
        let mut url = self.object_url(key)?;
        url.query_pairs_mut().append_pair("comp", "blocklist");
        let status = self
            .request(
                reqwest::Method::PUT,
                url,
                Some(reqwest::Body::from(body.clone())),
                Some(body.len() as u64),
                Some("application/xml"),
                &[],
            )
            .await?;
        ensure_success(status, "commit Azure block list")
    }

    fn signed_object_url(&self, key: &str, permissions: &str) -> Result<String> {
        let url = self.object_url(key)?;
        if self.profile.auth == AzureAuth::Sas {
            return Ok(self.with_sas(url)?.to_string());
        }
        let now = OffsetDateTime::now_utc();
        let expiry = now
            .checked_add(Duration::seconds(self.profile.presign_ttl_seconds.into()))
            .context("Azure SAS expiry is out of range")?;
        let protocol = if self.endpoint.scheme() == "https" {
            "https"
        } else {
            "https,http"
        };
        let canonical_resource = self.canonical_resource(&url)?;
        let string_to_sign = [
            permissions,
            "",
            &expiry.format(SAS_TIME)?,
            &canonical_resource,
            "",
            "",
            protocol,
            SERVICE_VERSION,
            "b",
            "",
            "",
            "",
            "",
            "",
            "",
            "",
        ]
        .join("\n");
        let signature = hmac_base64(&self.profile.account_key, &string_to_sign)?;
        let mut signed = url;
        signed
            .query_pairs_mut()
            .append_pair("sp", permissions)
            .append_pair("se", &expiry.format(SAS_TIME)?)
            .append_pair("spr", protocol)
            .append_pair("sv", SERVICE_VERSION)
            .append_pair("sr", "b")
            .append_pair("sig", &signature);
        Ok(signed.to_string())
    }

    async fn request(
        &self,
        method: reqwest::Method,
        url: Url,
        body: Option<reqwest::Body>,
        content_length: Option<u64>,
        content_type: Option<&str>,
        extra_x_ms: &[(&str, &str)],
    ) -> Result<u16> {
        let now = OffsetDateTime::now_utc();
        let mut headers = BTreeMap::new();
        headers.insert("x-ms-date".to_string(), now.format(HTTP_DATE)?);
        headers.insert("x-ms-version".to_string(), SERVICE_VERSION.to_string());
        for (name, value) in extra_x_ms {
            headers.insert(name.to_ascii_lowercase(), (*value).to_string());
        }
        let request_url = self.with_sas(url)?;
        let mut request = self.http.request(method.clone(), request_url.clone());
        for (name, value) in &headers {
            request = request.header(name, value);
        }
        if let Some(length) = content_length {
            request = request.header(reqwest::header::CONTENT_LENGTH, length.to_string());
        }
        if let Some(content_type) = content_type {
            request = request.header(reqwest::header::CONTENT_TYPE, content_type);
        }
        if self.profile.auth == AzureAuth::SharedKey {
            request = request.header(
                reqwest::header::AUTHORIZATION,
                self.authorization(
                    method.as_str(),
                    &request_url,
                    content_length,
                    content_type,
                    &headers,
                )?,
            );
        }
        let response = request
            .body(body.unwrap_or_else(|| reqwest::Body::from(Vec::new())))
            .send()
            .await
            .context("send Azure Blob request")?;
        let status = response.status().as_u16();
        if !(200..300).contains(&status) && status != 404 {
            let body = response.bytes().await.unwrap_or_default();
            let detail = String::from_utf8_lossy(&body[..body.len().min(1024)]);
            if detail.is_empty() {
                bail!("Azure request failed with status {status}");
            }
            bail!("Azure request failed with status {status}: {detail}");
        }
        Ok(status)
    }

    fn object_url(&self, key: &str) -> Result<Url> {
        let mut url = self.endpoint.clone();
        let mut segments = url
            .path_segments_mut()
            .map_err(|_| anyhow::anyhow!("Azure endpoint cannot be a base URL"))?;
        segments.pop_if_empty();
        segments.push(self.profile.container.trim());
        for segment in key.trim_matches('/').split('/') {
            if segment.is_empty() || matches!(segment, "." | "..") {
                bail!("invalid Azure object path {key}");
            }
            segments.push(segment);
        }
        drop(segments);
        Ok(url)
    }

    fn with_sas(&self, mut url: Url) -> Result<Url> {
        if self.profile.auth != AzureAuth::Sas {
            return Ok(url);
        }
        for (key, value) in url::form_urlencoded::parse(
            self.profile
                .sas_token
                .trim()
                .trim_start_matches('?')
                .as_bytes(),
        ) {
            url.query_pairs_mut().append_pair(&key, &value);
        }
        Ok(url)
    }

    fn authorization(
        &self,
        method: &str,
        url: &Url,
        content_length: Option<u64>,
        content_type: Option<&str>,
        headers: &BTreeMap<String, String>,
    ) -> Result<String> {
        let content_length = content_length
            .filter(|length| *length > 0)
            .map(|length| length.to_string())
            .unwrap_or_default();
        let canonical_headers = headers
            .iter()
            .map(|(name, value)| format!("{}:{}", name.to_ascii_lowercase(), value.trim()))
            .collect::<Vec<_>>()
            .join("\n");
        let string_to_sign = format!(
            "{method}\n\n\n{content_length}\n\n{}\n\n\n\n\n\n\n{canonical_headers}\n{}",
            content_type.unwrap_or_default(),
            self.canonical_resource(url)?,
        );
        let signature = hmac_base64(&self.profile.account_key, &string_to_sign)?;
        Ok(format!(
            "SharedKey {}:{signature}",
            self.profile.account_name
        ))
    }

    fn canonical_resource(&self, url: &Url) -> Result<String> {
        let path = percent_decode_str(url.path())
            .decode_utf8()
            .context("decode Azure object path")?;
        let mut out = format!("/{}{}", self.profile.account_name.trim(), path);
        let mut queries = BTreeMap::<String, Vec<String>>::new();
        for (key, value) in url.query_pairs() {
            queries
                .entry(key.to_ascii_lowercase())
                .or_default()
                .push(value.into_owned());
        }
        for (key, mut values) in queries {
            values.sort();
            out.push('\n');
            out.push_str(&key);
            out.push(':');
            out.push_str(&values.join(","));
        }
        Ok(out)
    }
}

fn block_size(size: u64) -> Result<u64> {
    let maximum_size = MAX_BLOCK_BYTES
        .checked_mul(MAX_BLOCKS)
        .expect("Azure block limit fits u64");
    if size > maximum_size {
        bail!("Azure Block Blob supports at most {maximum_size} bytes per object");
    }
    let required = (size + MAX_BLOCKS - 1) / MAX_BLOCKS;
    let rounded = required.max(MIN_BLOCK_BYTES).div_ceil(MIB) * MIB;
    Ok(rounded.min(MAX_BLOCK_BYTES))
}

fn block_id(index: u64) -> String {
    STANDARD.encode(index.to_be_bytes())
}

fn ensure_success(status: u16, action: &str) -> Result<()> {
    if (200..300).contains(&status) {
        Ok(())
    } else {
        bail!("{action} failed with status {status}")
    }
}

fn hmac_base64(account_key: &str, value: &str) -> Result<String> {
    let key = STANDARD
        .decode(account_key.trim())
        .context("decode Azure account key as base64")?;
    let mut mac = HmacSha256::new_from_slice(&key).context("create Azure HMAC")?;
    mac.update(value.as_bytes());
    Ok(STANDARD.encode(mac.finalize().into_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    fn shared_profile() -> AzureProfile {
        AzureProfile {
            auth: AzureAuth::SharedKey,
            account_name: "account".to_string(),
            container: "files".to_string(),
            endpoint: Some("https://account.blob.core.windows.net".to_string()),
            account_key: STANDARD.encode("secret"),
            sas_token: String::new(),
            prefix: "ii/".to_string(),
            presign_ttl_seconds: 60,
        }
    }

    #[test]
    fn block_size_covers_the_full_azure_limit() {
        assert_eq!(block_size(1).unwrap(), MIN_BLOCK_BYTES);
        assert_eq!(
            block_size(MAX_BLOCK_BYTES * MAX_BLOCKS).unwrap(),
            MAX_BLOCK_BYTES
        );
        assert!(block_size(MAX_BLOCK_BYTES * MAX_BLOCKS + 1).is_err());
    }

    #[test]
    fn block_ids_are_fixed_size_and_valid_base64() {
        let first = block_id(0);
        let last = block_id(MAX_BLOCKS - 1);
        assert_eq!(STANDARD.decode(&first).unwrap().len(), 8);
        assert_eq!(STANDARD.decode(&last).unwrap().len(), 8);
        assert_ne!(first, last);
    }

    #[test]
    fn shared_key_urls_are_object_scoped() {
        crate::install_crypto_provider();
        let client = AzureClient::new(&shared_profile()).unwrap();
        let url = client.signed_object_url("ii/file.txt", "r").unwrap();
        assert!(url.contains("/files/ii/file.txt?"));
        assert!(url.contains("sp=r"));
        assert!(url.contains("sr=b"));
        assert!(url.contains("sig="));
    }

    #[test]
    fn canonical_resource_includes_sorted_operation_query() {
        crate::install_crypto_provider();
        let client = AzureClient::new(&shared_profile()).unwrap();
        let mut url = client.object_url("ii/file.txt").unwrap();
        url.query_pairs_mut()
            .append_pair("blockid", "z")
            .append_pair("comp", "block");
        assert_eq!(
            client.canonical_resource(&url).unwrap(),
            "/account/files/ii/file.txt\nblockid:z\ncomp:block"
        );
    }

    #[test]
    fn shared_key_authorization_has_a_stable_canonical_request() {
        crate::install_crypto_provider();
        let client = AzureClient::new(&shared_profile()).unwrap();
        let url = Url::parse(
            "https://account.blob.core.windows.net/files/ii/file.txt?comp=block&blockid=z",
        )
        .unwrap();
        let headers = BTreeMap::from([
            (
                "x-ms-date".to_string(),
                "Wed, 23 Sep 2009 12:35:00 GMT".to_string(),
            ),
            ("x-ms-version".to_string(), SERVICE_VERSION.to_string()),
        ]);

        assert_eq!(
            client
                .authorization(
                    "PUT",
                    &url,
                    Some(4),
                    Some("application/octet-stream"),
                    &headers,
                )
                .unwrap(),
            "SharedKey account:/tKtBXZEkbd0pSsdvwlk6Kb/zkRXh/H+XS+jCKLdX38="
        );
    }

    #[tokio::test]
    async fn upload_uses_block_blob_requests_and_signed_urls_work_with_range_and_delete() {
        crate::install_crypto_provider();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let mut requests = Vec::new();
            for response in [
                ("404 Not Found", Vec::new()),
                ("201 Created", Vec::new()),
                ("201 Created", Vec::new()),
                ("206 Partial Content", b"cde".to_vec()),
                ("202 Accepted", Vec::new()),
            ] {
                let (mut stream, _) = listener.accept().await.unwrap();
                let request = read_mock_request(&mut stream).await;
                let headers = format!(
                    "HTTP/1.1 {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    response.0,
                    response.1.len()
                );
                stream.write_all(headers.as_bytes()).await.unwrap();
                stream.write_all(&response.1).await.unwrap();
                stream.shutdown().await.unwrap();
                requests.push(request);
            }
            requests
        });
        let source_dir = tempfile::tempdir().unwrap();
        let source_path = source_dir.path().join("source.bin");
        std::fs::write(&source_path, b"abcde").unwrap();
        let source = Source::from_file(source_path, None).await.unwrap();
        let mut profile = shared_profile();
        profile.endpoint = Some(format!("http://127.0.0.1:{port}"));

        let upload = upload(&source, &profile, true, false, None).await.unwrap();
        let download_url = upload.download_url.clone();
        let downloaded = tokio::task::spawn_blocking(move || -> Result<Vec<u8>> {
            let mut response = crate::backend::s3::get(&download_url, 2)?;
            let mut bytes = Vec::new();
            response
                .read_to_end(&mut bytes)
                .context("read mock Azure range")?;
            Ok(bytes)
        })
        .await
        .unwrap()
        .unwrap();
        assert_eq!(downloaded, b"cde");

        let delete_url = upload.delete_url.unwrap();
        tokio::task::spawn_blocking(move || {
            attohttpc::delete(&delete_url).send().unwrap();
        })
        .await
        .unwrap();

        let requests = server.await.unwrap();
        assert_eq!(
            requests
                .iter()
                .map(|request| request.method.as_str())
                .collect::<Vec<_>>(),
            ["HEAD", "PUT", "PUT", "GET", "DELETE"]
        );
        assert!(requests[0].target.starts_with("/files/ii/"));
        assert!(
            requests[0]
                .headers
                .get("authorization")
                .is_some_and(|value| value.starts_with("SharedKey account:"))
        );
        assert!(requests[1].target.contains("comp=block"));
        assert!(requests[1].target.contains("blockid="));
        assert_eq!(requests[1].body, b"abcde");
        assert!(requests[2].target.contains("comp=blocklist"));
        assert!(String::from_utf8_lossy(&requests[2].body).contains("<BlockList>"));
        assert_eq!(
            requests[3].headers.get("range").map(String::as_str),
            Some("bytes=2-")
        );
        assert!(requests[3].target.contains("sp=r"));
        assert!(requests[4].target.contains("sp=d"));
    }

    struct MockRequest {
        method: String,
        target: String,
        headers: BTreeMap<String, String>,
        body: Vec<u8>,
    }

    async fn read_mock_request(stream: &mut tokio::net::TcpStream) -> MockRequest {
        let mut bytes = Vec::new();
        let header_end = loop {
            let mut chunk = [0_u8; 1024];
            let read = stream.read(&mut chunk).await.unwrap();
            assert_ne!(read, 0, "mock Azure client closed request early");
            bytes.extend_from_slice(&chunk[..read]);
            if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                break index + 4;
            }
        };
        let header_text = std::str::from_utf8(&bytes[..header_end]).unwrap();
        let mut lines = header_text.split("\r\n");
        let mut request_line = lines.next().unwrap().split_whitespace();
        let method = request_line.next().unwrap().to_string();
        let target = request_line.next().unwrap().to_string();
        let headers = lines
            .filter_map(|line| line.split_once(':'))
            .map(|(name, value)| (name.to_ascii_lowercase(), value.trim().to_string()))
            .collect::<BTreeMap<_, _>>();
        let content_length = headers
            .get("content-length")
            .map(|value| value.parse::<usize>().unwrap())
            .unwrap_or_default();
        while bytes.len() - header_end < content_length {
            let mut chunk = [0_u8; 1024];
            let read = stream.read(&mut chunk).await.unwrap();
            assert_ne!(read, 0, "mock Azure client sent a short request body");
            bytes.extend_from_slice(&chunk[..read]);
        }
        MockRequest {
            method,
            target,
            headers,
            body: bytes[header_end..header_end + content_length].to_vec(),
        }
    }
}
