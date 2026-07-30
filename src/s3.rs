use std::{fmt::Write as _, io::Read, time::Duration};

use anyhow::{Context, Result, bail};
use attohttpc::{Session, header::HeaderName};
use hmac::{Hmac, Mac};
use percent_encoding::{AsciiSet, CONTROLS, percent_decode_str, utf8_percent_encode};
use sha2::{Digest, Sha256};
use time::{OffsetDateTime, format_description::FormatItem, macros::format_description};
use url::Url;

const CHUNK_SIZE: usize = 8 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const LONG_DATE: &[FormatItem<'static>] =
    format_description!("[year][month][day]T[hour][minute][second]Z");
const SHORT_DATE: &[FormatItem<'static>] = format_description!("[year][month][day]");

const S3_ENCODE: &AsciiSet = &CONTROLS
    .add(b':')
    .add(b'?')
    .add(b'#')
    .add(b'[')
    .add(b']')
    .add(b'@')
    .add(b'!')
    .add(b'$')
    .add(b'&')
    .add(b'\'')
    .add(b'(')
    .add(b')')
    .add(b'*')
    .add(b'+')
    .add(b',')
    .add(b';')
    .add(b'=')
    .add(b'"')
    .add(b' ')
    .add(b'<')
    .add(b'>')
    .add(b'%')
    .add(b'{')
    .add(b'}')
    .add(b'|')
    .add(b'\\')
    .add(b'^')
    .add(b'`');
const S3_ENCODE_SLASH: &AsciiSet = &S3_ENCODE.add(b'/');

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone, Debug)]
pub struct Client {
    bucket: String,
    region: String,
    endpoint: Url,
    endpoint_authority: String,
    access_key: String,
    secret_key: String,
    path_style: bool,
}

impl Client {
    pub fn new(
        bucket: &str,
        region: &str,
        endpoint: &str,
        access_key: &str,
        secret_key: &str,
        path_style: bool,
    ) -> Result<Self> {
        if bucket.is_empty() || region.is_empty() || access_key.is_empty() || secret_key.is_empty()
        {
            bail!("S3 bucket, region, access key, and secret key must not be empty");
        }
        let endpoint_authority = endpoint
            .split_once("://")
            .and_then(|(_, rest)| rest.split(['/', '?', '#']).next())
            .filter(|authority| !authority.is_empty())
            .context("read S3 endpoint authority")?
            .to_owned();
        let endpoint = Url::parse(endpoint).context("parse S3 endpoint")?;
        if !matches!(endpoint.scheme(), "http" | "https") || endpoint.host_str().is_none() {
            bail!("S3 endpoint must be an absolute HTTP or HTTPS URL");
        }
        if !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
        {
            bail!("S3 endpoint must not contain user info, a query, or a fragment");
        }
        Ok(Self {
            bucket: bucket.to_owned(),
            region: region.to_owned(),
            endpoint,
            endpoint_authority,
            access_key: access_key.to_owned(),
            secret_key: secret_key.to_owned(),
            path_style,
        })
    }

    pub fn head_object(&self, key: &str) -> Result<((), u16)> {
        let response = self.request("HEAD", key, &[], None, &[])?;
        Ok(((), response.status))
    }

    pub fn put_object_stream<R: Read>(&self, reader: &mut R, key: &str) -> Result<u16> {
        let first = read_chunk(reader)?;
        if first.len() < CHUNK_SIZE {
            return self.put_object(key, &first, "application/octet-stream");
        }

        let upload_id = self.initiate_multipart(key)?;
        let result = self.put_multipart(reader, key, first, &upload_id);
        if result.is_err() {
            let _ = self.abort_multipart(key, &upload_id);
        }
        result
    }

    pub fn presign_get(
        &self,
        key: &str,
        expiry_seconds: u32,
        _queries: Option<()>,
    ) -> Result<String> {
        self.presign("GET", key, expiry_seconds)
    }

    pub fn presign_delete(&self, key: &str, expiry_seconds: u32) -> Result<String> {
        self.presign("DELETE", key, expiry_seconds)
    }

    fn put_object(&self, key: &str, body: &[u8], content_type: &str) -> Result<u16> {
        let response = self.request("PUT", key, body, Some(content_type), &[])?;
        ensure_success(&response, "upload to S3")?;
        Ok(response.status)
    }

    fn initiate_multipart(&self, key: &str) -> Result<String> {
        let response = self.request(
            "POST",
            key,
            &[],
            Some("application/octet-stream"),
            &[("uploads".to_owned(), String::new())],
        )?;
        ensure_success(&response, "start S3 multipart upload")?;
        xml_tag(&response.body, "UploadId").context("read S3 multipart upload id")
    }

    fn put_multipart(
        &self,
        reader: &mut impl Read,
        key: &str,
        mut chunk: Vec<u8>,
        upload_id: &str,
    ) -> Result<u16> {
        let mut parts = Vec::new();
        let mut number = 1_u32;
        loop {
            let query = [
                ("partNumber".to_owned(), number.to_string()),
                ("uploadId".to_owned(), upload_id.to_owned()),
            ];
            let response =
                self.request("PUT", key, &chunk, Some("application/octet-stream"), &query)?;
            ensure_success(&response, "upload S3 multipart part")?;
            let etag = response
                .headers
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case("etag"))
                .map(|(_, value)| value.clone())
                .context("S3 multipart response is missing ETag")?;
            parts.push((number, etag));

            chunk = read_chunk(reader)?;
            if chunk.is_empty() {
                break;
            }
            number += 1;
        }

        let mut body = String::from("<CompleteMultipartUpload>");
        for (number, etag) in parts {
            write!(
                body,
                "<Part><PartNumber>{number}</PartNumber><ETag>{}</ETag></Part>",
                xml_escape(&etag)
            )?;
        }
        body.push_str("</CompleteMultipartUpload>");
        let query = [("uploadId".to_owned(), upload_id.to_owned())];
        let response = self.request(
            "POST",
            key,
            body.as_bytes(),
            Some("application/xml"),
            &query,
        )?;
        ensure_success(&response, "complete S3 multipart upload")?;
        Ok(response.status)
    }

    fn abort_multipart(&self, key: &str, upload_id: &str) -> Result<()> {
        let query = [("uploadId".to_owned(), upload_id.to_owned())];
        let response = self.request("DELETE", key, &[], None, &query)?;
        ensure_success(&response, "abort S3 multipart upload")
    }

    fn presign(&self, method: &str, key: &str, expiry_seconds: u32) -> Result<String> {
        if expiry_seconds > 604_800 {
            bail!("S3 presign TTL must not exceed 604800 seconds");
        }
        let now = OffsetDateTime::now_utc();
        let scope = self.scope(now)?;
        let query = vec![
            ("X-Amz-Algorithm".to_owned(), "AWS4-HMAC-SHA256".to_owned()),
            (
                "X-Amz-Credential".to_owned(),
                format!("{}/{}", self.access_key, scope),
            ),
            ("X-Amz-Date".to_owned(), now.format(LONG_DATE)?),
            ("X-Amz-Expires".to_owned(), expiry_seconds.to_string()),
            ("X-Amz-SignedHeaders".to_owned(), "host".to_owned()),
        ];
        let path = self.object_path(key);
        let host = self.host();
        let canonical = canonical_request(
            method,
            &path,
            &query,
            &[("host", host.as_str())],
            "UNSIGNED-PAYLOAD",
        );
        let signature = self.signature(now, &canonical)?;
        Ok(format!(
            "{}?{}&X-Amz-Signature={}",
            self.object_url(key),
            canonical_query(&query),
            signature
        ))
    }

    fn request(
        &self,
        method: &str,
        key: &str,
        body: &[u8],
        content_type: Option<&str>,
        query: &[(String, String)],
    ) -> Result<Response> {
        let query = query.to_vec();
        let now = OffsetDateTime::now_utc();
        let payload_hash = hex_sha256(body);
        let mut headers = vec![
            ("host".to_owned(), self.host()),
            ("x-amz-content-sha256".to_owned(), payload_hash.clone()),
            ("x-amz-date".to_owned(), now.format(LONG_DATE)?),
        ];
        if let Some(content_type) = content_type {
            headers.push(("content-length".to_owned(), body.len().to_string()));
            headers.push(("content-type".to_owned(), content_type.to_owned()));
        }
        let canonical = canonical_request(
            method,
            &self.object_path(key),
            &query,
            &header_refs(&headers),
            &payload_hash,
        );
        headers.push((
            "authorization".to_owned(),
            self.authorization(now, &headers, &canonical)?,
        ));

        let mut session = Session::new();
        session.timeout(REQUEST_TIMEOUT);
        for (name, value) in &headers {
            session.header(HeaderName::from_bytes(name.as_bytes())?, value);
        }
        let url = with_query(self.object_url(key), &query);
        let response = match method {
            "HEAD" => session.head(url).bytes(&[]).send()?,
            "PUT" => session.put(url).bytes(body).send()?,
            "POST" => session.post(url).bytes(body).send()?,
            "DELETE" => session.delete(url).bytes(&[]).send()?,
            _ => unreachable!("unsupported S3 method"),
        };
        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .map(|(name, value)| Ok((name.to_string(), value.to_str()?.to_owned())))
            .collect::<Result<Vec<_>>>()?;
        let body = if method == "HEAD" {
            Vec::new()
        } else {
            response.bytes()?
        };
        Ok(Response {
            status,
            headers,
            body,
        })
    }

    fn authorization(
        &self,
        now: OffsetDateTime,
        headers: &[(String, String)],
        canonical: &str,
    ) -> Result<String> {
        Ok(format!(
            "AWS4-HMAC-SHA256 Credential={}/{},SignedHeaders={},Signature={}",
            self.access_key,
            self.scope(now)?,
            signed_headers(&header_refs(headers)),
            self.signature(now, canonical)?
        ))
    }

    fn signature(&self, now: OffsetDateTime, canonical: &str) -> Result<String> {
        let scope = self.scope(now)?;
        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n{}\n{}\n{}",
            now.format(LONG_DATE)?,
            scope,
            hex_sha256(canonical.as_bytes())
        );
        let date = now.format(SHORT_DATE)?;
        let date_key = hmac(
            format!("AWS4{}", self.secret_key).as_bytes(),
            date.as_bytes(),
        )?;
        let region_key = hmac(&date_key, self.region.as_bytes())?;
        let service_key = hmac(&region_key, b"s3")?;
        let signing_key = hmac(&service_key, b"aws4_request")?;
        Ok(hex_encode(&hmac(&signing_key, string_to_sign.as_bytes())?))
    }

    fn scope(&self, now: OffsetDateTime) -> Result<String> {
        Ok(format!(
            "{}/{}/s3/aws4_request",
            now.format(SHORT_DATE)?,
            self.region
        ))
    }

    fn host(&self) -> String {
        if self.path_style {
            self.endpoint_authority.clone()
        } else {
            format!("{}.{}", self.bucket, self.endpoint_authority)
        }
    }

    fn object_path(&self, key: &str) -> String {
        let base = self.endpoint.path().trim_end_matches('/');
        let key = key.trim_start_matches('/');
        let key = uri_encode(key, false);
        if self.path_style {
            format!("{base}/{}/{}", uri_encode(&self.bucket, true), key)
        } else {
            format!("{base}/{key}")
        }
    }

    fn object_url(&self, key: &str) -> String {
        format!(
            "{}://{}{}",
            self.endpoint.scheme(),
            self.host(),
            self.object_path(key)
        )
    }
}

struct Response {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

fn read_chunk(reader: &mut impl Read) -> Result<Vec<u8>> {
    let mut chunk = vec![0; CHUNK_SIZE];
    let mut used = 0;
    while used < chunk.len() {
        let read = reader.read(&mut chunk[used..])?;
        if read == 0 {
            break;
        }
        used += read;
    }
    chunk.truncate(used);
    Ok(chunk)
}

fn ensure_success(response: &Response, action: &str) -> Result<()> {
    if (200..300).contains(&response.status) {
        return Ok(());
    }
    let body = String::from_utf8_lossy(&response.body);
    bail!(
        "{action} failed with status {}{}",
        response.status,
        if body.is_empty() {
            String::new()
        } else {
            format!(": {body}")
        }
    );
}

fn xml_tag(bytes: &[u8], tag: &str) -> Option<String> {
    let text = std::str::from_utf8(bytes).ok()?;
    let start = format!("<{tag}>");
    let end = format!("</{tag}>");
    let value = text.split_once(&start)?.1.split_once(&end)?.0;
    Some(xml_unescape(value))
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn xml_unescape(value: &str) -> String {
    value
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

fn with_query(url: String, query: &[(String, String)]) -> String {
    if query.is_empty() {
        url
    } else {
        format!("{url}?{}", canonical_query(query))
    }
}

fn uri_encode(value: &str, encode_slash: bool) -> String {
    utf8_percent_encode(
        value,
        if encode_slash {
            S3_ENCODE_SLASH
        } else {
            S3_ENCODE
        },
    )
    .to_string()
}

fn canonical_query(query: &[(String, String)]) -> String {
    let mut pairs = query.to_vec();
    pairs.sort();
    pairs
        .into_iter()
        .map(|(key, value)| format!("{}={}", uri_encode(&key, true), uri_encode(&value, true)))
        .collect::<Vec<_>>()
        .join("&")
}

fn canonical_request(
    method: &str,
    path: &str,
    query: &[(String, String)],
    headers: &[(&str, &str)],
    payload_hash: &str,
) -> String {
    let decoded = percent_decode_str(path).decode_utf8_lossy();
    format!(
        "{method}\n{}\n{}\n{}\n\n{}\n{payload_hash}",
        uri_encode(&decoded, false),
        canonical_query(query),
        canonical_headers(headers),
        signed_headers(headers)
    )
}

fn canonical_headers(headers: &[(&str, &str)]) -> String {
    let mut headers = headers
        .iter()
        .map(|(name, value)| (name.to_ascii_lowercase(), value.trim()))
        .collect::<Vec<_>>();
    headers.sort_by(|left, right| left.0.cmp(&right.0));
    headers
        .into_iter()
        .map(|(name, value)| format!("{name}:{value}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn signed_headers(headers: &[(&str, &str)]) -> String {
    let mut names = headers
        .iter()
        .map(|(name, _)| name.to_ascii_lowercase())
        .collect::<Vec<_>>();
    names.sort();
    names.join(";")
}

fn header_refs(headers: &[(String, String)]) -> Vec<(&str, &str)> {
    headers
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_str()))
        .collect()
}

fn hmac(key: &[u8], data: &[u8]) -> Result<Vec<u8>> {
    let mut mac = HmacSha256::new_from_slice(key)?;
    mac.update(data);
    Ok(mac.finalize().into_bytes().to_vec())
}

fn hex_sha256(data: &[u8]) -> String {
    hex_encode(&Sha256::digest(data))
}

fn hex_encode(data: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(data.len() * 2);
    for byte in data {
        value.push(HEX[(byte >> 4) as usize] as char);
        value.push(HEX[(byte & 15) as usize] as char);
    }
    value
}

#[cfg(test)]
mod tests {
    use std::{
        io::{BufRead as _, BufReader, Read as _, Write as _},
        net::TcpListener,
        thread,
    };

    use super::*;

    #[test]
    fn signs_path_style_and_virtual_host_urls() {
        let path_style = Client::new(
            "bucket",
            "us-east-1",
            "http://localhost:9000",
            "access",
            "secret",
            true,
        )
        .unwrap();
        assert_eq!(
            path_style.object_url("dir/a b"),
            "http://localhost:9000/bucket/dir/a%20b"
        );
        let virtual_host = Client::new(
            "bucket",
            "us-east-1",
            "https://s3.example.test",
            "access",
            "secret",
            false,
        )
        .unwrap();
        assert_eq!(
            virtual_host.object_url("dir/a b"),
            "https://bucket.s3.example.test/dir/a%20b"
        );
        let standard_port = Client::new(
            "bucket",
            "us-east-1",
            "https://s3.example.test:443",
            "access",
            "secret",
            false,
        )
        .unwrap();
        assert_eq!(
            standard_port.object_url("key"),
            "https://bucket.s3.example.test:443/key"
        );
    }

    #[test]
    fn client_rejects_incomplete_or_unsafe_configuration() {
        let incomplete = Client::new(
            "",
            "us-east-1",
            "https://s3.example.test",
            "access",
            "secret",
            true,
        )
        .unwrap_err();
        assert!(incomplete.to_string().contains("must not be empty"));

        let unsupported_scheme = Client::new(
            "bucket",
            "us-east-1",
            "ftp://s3.example.test",
            "access",
            "secret",
            true,
        )
        .unwrap_err();
        assert!(
            unsupported_scheme
                .to_string()
                .contains("absolute HTTP or HTTPS")
        );

        let credentials = Client::new(
            "bucket",
            "us-east-1",
            "https://user:password@s3.example.test",
            "access",
            "secret",
            true,
        )
        .unwrap_err();
        assert!(
            credentials
                .to_string()
                .contains("must not contain user info")
        );
    }

    #[test]
    fn paths_preserve_endpoint_prefix_and_encode_object_keys() {
        let client = Client::new(
            "bucket/name",
            "us-east-1",
            "https://s3.example.test/api/",
            "access",
            "secret",
            true,
        )
        .unwrap();

        assert_eq!(
            client.object_url("/folder/100% ready?#"),
            "https://s3.example.test/api/bucket%2Fname/folder/100%25%20ready%3F%23"
        );
    }

    #[test]
    fn canonical_query_sorts_and_encodes() {
        let query = vec![
            ("b".to_owned(), "a b".to_owned()),
            ("a".to_owned(), "/".to_owned()),
        ];
        assert_eq!(canonical_query(&query), "a=%2F&b=a%20b");
    }

    #[test]
    fn sigv4_matches_aws_s3_reference_vector() {
        let client = Client::new(
            "examplebucket",
            "us-east-1",
            "https://s3.amazonaws.com",
            "AKIAIOSFODNN7EXAMPLE",
            "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
            false,
        )
        .unwrap();
        let headers = [
            ("host", "examplebucket.s3.amazonaws.com"),
            ("range", "bytes=0-9"),
            (
                "x-amz-content-sha256",
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            ),
            ("x-amz-date", "20130524T000000Z"),
        ];
        let canonical = canonical_request(
            "GET",
            "/test.txt",
            &[],
            &headers,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        );
        assert_eq!(
            canonical,
            "GET\n/test.txt\n\nhost:examplebucket.s3.amazonaws.com\nrange:bytes=0-9\nx-amz-content-sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855\nx-amz-date:20130524T000000Z\n\nhost;range;x-amz-content-sha256;x-amz-date\ne3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        let now = time::macros::datetime!(2013-05-24 00:00 UTC);
        assert_eq!(
            client.signature(now, &canonical).unwrap(),
            "f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41"
        );
    }

    #[test]
    fn parses_and_escapes_multipart_xml() {
        assert_eq!(
            xml_tag(b"<UploadId>a&amp;b</UploadId>", "UploadId").as_deref(),
            Some("a&b")
        );
        assert_eq!(xml_escape("\"<&"), "&quot;&lt;&amp;");
    }

    #[test]
    fn reports_unsuccessful_responses_with_body() {
        let response = Response {
            status: 403,
            headers: Vec::new(),
            body: b"AccessDenied".to_vec(),
        };

        let error = ensure_success(&response, "download from S3").unwrap_err();
        assert_eq!(
            error.to_string(),
            "download from S3 failed with status 403: AccessDenied"
        );
    }

    #[test]
    fn multipart_upload_uses_s3_request_sequence() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let mut requests = Vec::new();
            for (etag, response_body) in [
                (
                    None,
                    "<InitiateMultipartUploadResult><UploadId>upload&amp;id</UploadId></InitiateMultipartUploadResult>",
                ),
                (Some("\"part-1\""), ""),
                (Some("\"part-2\""), ""),
                (None, ""),
            ] {
                let (stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request_line = String::new();
                reader.read_line(&mut request_line).unwrap();
                let mut content_length = 0;
                let mut authorization = String::new();
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" {
                        break;
                    }
                    let (name, value) = line.split_once(':').unwrap();
                    if name.eq_ignore_ascii_case("content-length") {
                        content_length = value.trim().parse().unwrap();
                    }
                    if name.eq_ignore_ascii_case("authorization") {
                        authorization = value.trim().to_owned();
                    }
                }
                let mut body = vec![0; content_length];
                reader.read_exact(&mut body).unwrap();
                requests.push((request_line, authorization, body));
                let mut stream = stream;
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n",
                    response_body.len()
                )
                .unwrap();
                if let Some(etag) = etag {
                    write!(stream, "ETag: {etag}\r\n").unwrap();
                }
                stream.write_all(b"\r\n").unwrap();
                stream.write_all(response_body.as_bytes()).unwrap();
            }
            requests
        });

        let client = Client::new(
            "bucket",
            "us-east-1",
            &format!("http://{address}"),
            "access",
            "secret",
            true,
        )
        .unwrap();
        let mut source = vec![7_u8; CHUNK_SIZE];
        source.push(9);
        assert_eq!(
            client
                .put_object_stream(&mut source.as_slice(), "dir/object")
                .unwrap(),
            200
        );

        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 4);
        assert!(
            requests[0]
                .0
                .starts_with("POST /bucket/dir/object?uploads= HTTP/1.1")
        );
        assert!(
            requests[0]
                .1
                .starts_with("AWS4-HMAC-SHA256 Credential=access/")
        );
        assert!(requests[1].0.contains("partNumber=1&uploadId=upload%26id"));
        assert_eq!(requests[1].2.len(), CHUNK_SIZE);
        assert!(requests[2].0.contains("partNumber=2&uploadId=upload%26id"));
        assert_eq!(requests[2].2, [9]);
        let complete = String::from_utf8(requests[3].2.clone()).unwrap();
        assert_eq!(
            complete,
            "<CompleteMultipartUpload><Part><PartNumber>1</PartNumber><ETag>&quot;part-1&quot;</ETag></Part><Part><PartNumber>2</PartNumber><ETag>&quot;part-2&quot;</ETag></Part></CompleteMultipartUpload>"
        );
    }
}
