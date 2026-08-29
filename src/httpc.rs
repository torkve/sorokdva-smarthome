//! Minimal HTTP/1.1 client over hyper + rustls, replacing reqwest: the app
//! only ever fetches a local influxdb over plain http and posts JSON to
//! dialogs.yandex.net over https.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use http::{header, HeaderName, HeaderValue, Method, Request, StatusCode};
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper_util::rt::TokioIo;
use serde_json::Value;
use tokio::net::TcpStream;
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::rustls::{ClientConfig, RootCertStore};
use tokio_rustls::TlsConnector;

#[derive(Debug)]
struct ParsedUrl {
    https: bool,
    host: String,
    port: u16,
    path_and_query: String,
}

impl ParsedUrl {
    /// RFC 9110 Host: authority with the port unless it is the default one.
    fn host_header(&self) -> String {
        if (self.https && self.port == 443) || (!self.https && self.port == 80) {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

fn parse_url(url: &str) -> Result<ParsedUrl> {
    let (https, rest) = if let Some(rest) = url.strip_prefix("https://") {
        (true, rest)
    } else if let Some(rest) = url.strip_prefix("http://") {
        (false, rest)
    } else {
        bail!("unsupported url scheme in {url}");
    };
    let (authority, path_and_query) = match rest.find('/') {
        Some(pos) => (&rest[..pos], &rest[pos..]),
        None => (rest, "/"),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => (
            host.to_string(),
            port.parse().with_context(|| format!("bad port in {url}"))?,
        ),
        None => (authority.to_string(), if https { 443 } else { 80 }),
    };
    if host.is_empty() {
        bail!("missing host in {url}");
    }
    Ok(ParsedUrl {
        https,
        host,
        port,
        path_and_query: path_and_query.to_string(),
    })
}

#[derive(Clone)]
pub struct HttpClient {
    connect_timeout: Duration,
    total_timeout: Duration,
    default_headers: Arc<Vec<(HeaderName, HeaderValue)>>,
    tls: TlsConnector,
}

impl HttpClient {
    pub fn new(
        connect_timeout: Duration,
        total_timeout: Duration,
        default_headers: Vec<(HeaderName, HeaderValue)>,
    ) -> Self {
        let mut roots = RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        // name the provider explicitly so a future dependency enabling
        // another rustls backend cannot make builder() ambiguous (which
        // would abort at startup under panic=immediate-abort)
        let config = ClientConfig::builder_with_provider(Arc::new(
            tokio_rustls::rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .expect("ring provider supports the default protocol versions")
        .with_root_certificates(roots)
        .with_no_client_auth();
        HttpClient {
            connect_timeout,
            total_timeout,
            default_headers: Arc::new(default_headers),
            tls: TlsConnector::from(Arc::new(config)),
        }
    }

    async fn request_inner(
        &self,
        method: Method,
        url: &ParsedUrl,
        content_type: Option<&'static str>,
        body: Bytes,
    ) -> Result<(StatusCode, Bytes)> {
        let stream = tokio::time::timeout(
            self.connect_timeout,
            TcpStream::connect((url.host.as_str(), url.port)),
        )
        .await
        .map_err(|_| anyhow!("connect timeout to {}:{}", url.host, url.port))?
        .with_context(|| format!("cannot connect to {}:{}", url.host, url.port))?;

        let mut builder = Request::builder()
            .method(method)
            .uri(&url.path_and_query)
            .header(header::HOST, url.host_header());
        for (name, value) in self.default_headers.iter() {
            builder = builder.header(name, value);
        }
        if let Some(content_type) = content_type {
            builder = builder.header(header::CONTENT_TYPE, content_type);
        }
        let request = builder.body(Full::new(body)).context("bad request")?;

        let response = if url.https {
            let server_name = ServerName::try_from(url.host.clone())
                .with_context(|| format!("bad server name {}", url.host))?;
            // the connect timeout covers connection establishment
            // including the TLS handshake
            let stream =
                tokio::time::timeout(self.connect_timeout, self.tls.connect(server_name, stream))
                    .await
                    .map_err(|_| anyhow!("tls handshake timeout with {}", url.host))?
                    .with_context(|| format!("tls handshake with {} failed", url.host))?;
            let (mut sender, conn) =
                hyper::client::conn::http1::handshake(TokioIo::new(stream)).await?;
            tokio::spawn(conn);
            sender.send_request(request).await?
        } else {
            let (mut sender, conn) =
                hyper::client::conn::http1::handshake(TokioIo::new(stream)).await?;
            tokio::spawn(conn);
            sender.send_request(request).await?
        };

        let status = response.status();
        let body = response.into_body().collect().await?.to_bytes();
        Ok((status, body))
    }

    pub async fn request(
        &self,
        method: Method,
        url: &str,
        content_type: Option<&'static str>,
        body: Bytes,
    ) -> Result<(StatusCode, Bytes)> {
        let url = parse_url(url)?;
        tokio::time::timeout(
            self.total_timeout,
            self.request_inner(method, &url, content_type, body),
        )
        .await
        .map_err(|_| anyhow!("request timeout"))?
    }

    pub async fn get_json(&self, url: &str) -> Result<Value> {
        let (status, body) = self.request(Method::GET, url, None, Bytes::new()).await?;
        if !status.is_success() {
            bail!("GET {url} failed: {status}");
        }
        serde_json::from_slice(&body).context("invalid json response")
    }

    /// POST a JSON payload; returns (status, parsed body or Null).
    pub async fn post_json(&self, url: &str, payload: &Value) -> Result<(StatusCode, Value)> {
        let body = Bytes::from(serde_json::to_vec(payload)?);
        let (status, body) = self
            .request(Method::POST, url, Some("application/json"), body)
            .await?;
        let data = serde_json::from_slice(&body).unwrap_or(Value::Null);
        Ok((status, data))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        let url = parse_url("http://localhost:8086/query?db=freezer").unwrap();
        assert!(!url.https);
        assert_eq!(url.host, "localhost");
        assert_eq!(url.port, 8086);
        assert_eq!(url.path_and_query, "/query?db=freezer");

        let url = parse_url("https://dialogs.yandex.net/api/v1/skills/x/callback/state").unwrap();
        assert!(url.https);
        assert_eq!(url.port, 443);
        assert_eq!(url.path_and_query, "/api/v1/skills/x/callback/state");

        assert!(parse_url("ftp://x/").is_err());
        assert!(parse_url("https:///path").is_err());

        assert_eq!(
            parse_url("http://localhost:8086/query")
                .unwrap()
                .host_header(),
            "localhost:8086"
        );
        assert_eq!(
            parse_url("http://localhost/query").unwrap().host_header(),
            "localhost"
        );
        assert_eq!(
            parse_url("https://dialogs.yandex.net/x")
                .unwrap()
                .host_header(),
            "dialogs.yandex.net"
        );
        assert_eq!(
            parse_url("https://example.com:8443/x")
                .unwrap()
                .host_header(),
            "example.com:8443"
        );
    }
}
