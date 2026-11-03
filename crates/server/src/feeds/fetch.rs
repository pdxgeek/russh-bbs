//! The guarded fetcher: resolve the name, check every resolved address, then connect to the
//! address that was checked (no second lookup, so DNS rebinding cannot swap it).
//!
//! Every hop of a redirect goes through the same steps. Caps: connect time, total time, response
//! size (streamed and stopped), redirects. No cookies are sent or kept. The `User-Agent` names
//! the software, not its owner. Compressed encodings are never requested, so a decompression
//! bomb cannot arrive. Proxies are never used (see [`proxy_in_environment`] and D89).

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use http_body_util::{BodyExt, Empty};
use hyper::body::Bytes;
use hyper::header::{ACCEPT, ACCEPT_ENCODING, CONNECTION, HOST, LOCATION, USER_AGENT};
use hyper::{Method, Request, StatusCode};
use hyper_util::rt::TokioIo;
use russh_bbs_core::feeds::{FeedUrl, Fetch, FetchError};
use rustls_pki_types::ServerName;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_rustls::TlsConnector;
use tracing::warn;
use url::Host;

use super::guard::{Guard, Refusal, Resolve, SystemResolver};

/// The fixed `User-Agent`: the software and its version, not the owner of the board.
pub const USER_AGENT_VALUE: &str = concat!("russh-bbs-feed-fetcher/", env!("CARGO_PKG_VERSION"));

/// Caps on one fetch.
#[derive(Debug, Clone, Copy)]
pub struct FetchLimits {
    /// Resolving, plus connecting (TCP and TLS), per hop.
    pub connect: Duration,
    /// The whole fetch, redirects included.
    pub total: Duration,
    /// Largest response body accepted, in bytes.
    pub max_bytes: usize,
    /// Most redirects followed.
    pub max_redirects: usize,
}

impl Default for FetchLimits {
    fn default() -> Self {
        Self {
            connect: Duration::from_secs(5),
            total: Duration::from_secs(20),
            max_bytes: 1024 * 1024,
            max_redirects: 3,
        }
    }
}

/// The names of the proxy variables the fetcher ignores.
const PROXY_VARIABLES: [&str; 6] = [
    "HTTPS_PROXY",
    "https_proxy",
    "HTTP_PROXY",
    "http_proxy",
    "ALL_PROXY",
    "all_proxy",
];

/// The proxy variables that are set. The fetcher never uses a proxy: a proxy resolves the name
/// itself, so the guard could not check the address that is actually connected to. A board behind
/// a mandatory proxy therefore cannot fetch feeds (it fails closed); the start-up warning says so.
#[must_use]
pub fn proxy_in_environment() -> Vec<&'static str> {
    PROXY_VARIABLES
        .into_iter()
        .filter(|name| std::env::var_os(name).is_some_and(|v| !v.is_empty()))
        .collect()
}

/// A fetcher that enforces the SSRF guard on every hop.
pub struct GuardedFetcher<R = SystemResolver> {
    resolver: R,
    guard: Guard,
    limits: FetchLimits,
    tls: Option<TlsConnector>,
}

impl<R> std::fmt::Debug for GuardedFetcher<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GuardedFetcher")
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}

/// Where the fetcher's trusted CA certificates come from.
#[derive(Debug, Clone, Default)]
pub enum Roots {
    /// The platform store (`SSL_CERT_FILE`, `SSL_CERT_DIR` or the OS default locations).
    #[default]
    Platform,
    /// Only these certificates, from `feeds.ca_file`: they replace the platform store for feed
    /// fetching and nothing else (the Postgres connection has its own `tls_root_cert`).
    Only(Vec<rustls_pki_types::CertificateDer<'static>>),
}

/// Why `feeds.ca_file` could not be used.
#[derive(Debug, thiserror::Error)]
pub enum CaFileError {
    /// The file could not be read.
    #[error("cannot read the file: {0}")]
    Read(std::io::Error),
    /// The file holds something that is not a PEM certificate.
    #[error("not a valid PEM certificate file: {0}")]
    Pem(rustls_pki_types::pem::Error),
    /// The file holds no certificate at all (an empty store would fail every feed).
    #[error("the file holds no PEM certificate")]
    Empty,
}

/// Read a PEM bundle for `feeds.ca_file`. Fails closed: unreadable, malformed or empty is an
/// error, never a silent fall back to the platform store.
///
/// # Errors
/// See [`CaFileError`].
pub fn load_ca_file(
    path: &std::path::Path,
) -> Result<Vec<rustls_pki_types::CertificateDer<'static>>, CaFileError> {
    use rustls_pki_types::pem::PemObject;
    let bytes = std::fs::read(path).map_err(CaFileError::Read)?;
    let certs = rustls_pki_types::CertificateDer::pem_slice_iter(&bytes)
        .collect::<Result<Vec<_>, _>>()
        .map_err(CaFileError::Pem)?;
    if certs.is_empty() {
        return Err(CaFileError::Empty);
    }
    Ok(certs)
}

/// TLS roots from `roots`, with the ring provider. `None` only if the provider refused its
/// own default protocol versions, in which case https feeds fail closed.
fn tls_connector(
    roots: &Roots,
    extra_roots: Vec<rustls_pki_types::CertificateDer<'static>>,
) -> Option<TlsConnector> {
    let mut store = rustls::RootCertStore::empty();
    match roots {
        Roots::Platform => {
            let loaded = rustls_native_certs::load_native_certs();
            store.add_parsable_certificates(loaded.certs);
        }
        Roots::Only(certs) => {
            store.add_parsable_certificates(certs.iter().cloned());
        }
    }
    for cert in extra_roots {
        let _ = store.add(cert);
    }
    if store.is_empty() {
        match roots {
            Roots::Platform => warn!(
                "no CA certificates found: https feeds cannot be verified and will not load. \
                 Set feeds.ca_file to a PEM file of the CAs to trust, or install a CA bundle \
                 (SSL_CERT_FILE) where the server runs"
            ),
            Roots::Only(_) => {
                warn!("feeds.ca_file holds no usable CA certificate: https feeds will not load");
            }
        }
    }
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .ok()?
    .with_root_certificates(store)
    .with_no_client_auth();
    Some(TlsConnector::from(Arc::new(config)))
}

impl GuardedFetcher<SystemResolver> {
    /// The production fetcher: system DNS, the platform's CA roots.
    #[must_use]
    pub fn new(limits: FetchLimits) -> Self {
        Self::with_resolver(SystemResolver, Guard::new(), limits)
    }
}

impl<R: Resolve> GuardedFetcher<R> {
    /// A fetcher with its own resolver and guard (tests).
    #[must_use]
    pub fn with_resolver(resolver: R, guard: Guard, limits: FetchLimits) -> Self {
        Self::with_roots(resolver, guard, limits, &Roots::Platform)
    }

    /// A fetcher that trusts `roots` (the platform store, or only the `feeds.ca_file`).
    #[must_use]
    pub fn with_roots(resolver: R, guard: Guard, limits: FetchLimits, roots: &Roots) -> Self {
        Self {
            resolver,
            guard,
            limits,
            tls: tls_connector(roots, Vec::new()),
        }
    }

    /// Trust these certificates as well (a test server's, generated at test time).
    #[cfg(test)]
    #[must_use]
    pub fn trusting(mut self, roots: Vec<rustls_pki_types::CertificateDer<'static>>) -> Self {
        self.tls = tls_connector(&Roots::Platform, roots);
        self
    }

    async fn run(&self, start: &FeedUrl, allow_http: bool) -> Result<Vec<u8>, FetchError> {
        let mut current = start.clone();
        for _ in 0..=self.limits.max_redirects {
            match self.hop(&current).await? {
                Reply::Body(bytes) => return Ok(bytes),
                Reply::Redirect(location) => {
                    let next = current
                        .join(&location, allow_http)
                        .map_err(|_| FetchError::Refused)?;
                    // Never step down from https to http, whatever the flag says.
                    if current.is_https() && !next.is_https() {
                        return Err(FetchError::Refused);
                    }
                    current = next;
                }
            }
        }
        // Too many redirects.
        Err(FetchError::Refused)
    }

    /// One request: guard, connect, send, read.
    async fn hop(&self, url: &FeedUrl) -> Result<Reply, FetchError> {
        let u = url.url();
        let port = u.port_or_known_default().ok_or(FetchError::Refused)?;
        let host = u.host().ok_or(FetchError::Refused)?;
        let addrs: Vec<SocketAddr> = match &host {
            Host::Ipv4(ip) => vec![SocketAddr::new(IpAddr::V4(*ip), port)],
            Host::Ipv6(ip) => vec![SocketAddr::new(IpAddr::V6(*ip), port)],
            Host::Domain(name) => timeout(self.limits.connect, self.resolver.resolve(name, port))
                .await
                .map_err(|_| FetchError::TimedOut)?
                .map_err(|_| FetchError::Failed)?,
        };
        // Every address is checked; the connection below uses these same addresses.
        self.guard.check(&addrs).map_err(|refusal| match refusal {
            Refusal::Address => FetchError::Refused,
            Refusal::NoAddress => FetchError::Failed,
        })?;
        let tcp = self.connect(&addrs).await?;
        let host_header = u.host_str().ok_or(FetchError::Refused)?;
        let host_header = match u.port() {
            Some(p) => format!("{host_header}:{p}"),
            None => host_header.to_owned(),
        };
        let mut target = u.path().to_owned();
        if let Some(query) = u.query() {
            target.push('?');
            target.push_str(query);
        }
        if url.is_https() {
            let name = match &host {
                Host::Domain(d) => {
                    ServerName::try_from((*d).to_owned()).map_err(|_| FetchError::Refused)?
                }
                Host::Ipv4(ip) => ServerName::IpAddress(IpAddr::V4(*ip).into()),
                Host::Ipv6(ip) => ServerName::IpAddress(IpAddr::V6(*ip).into()),
            };
            let connector = self.tls.as_ref().ok_or(FetchError::Failed)?;
            let tls = timeout(self.limits.connect, connector.connect(name, tcp))
                .await
                .map_err(|_| FetchError::TimedOut)?
                .map_err(|_| FetchError::Failed)?;
            self.exchange(tls, &host_header, &target).await
        } else {
            self.exchange(tcp, &host_header, &target).await
        }
    }

    /// Connect to the first of the already-checked addresses that answers.
    async fn connect(&self, addrs: &[SocketAddr]) -> Result<TcpStream, FetchError> {
        let mut timed_out = false;
        for addr in addrs {
            match timeout(self.limits.connect, TcpStream::connect(addr)).await {
                Ok(Ok(stream)) => return Ok(stream),
                Ok(Err(_)) => {}
                Err(_) => timed_out = true,
            }
        }
        Err(if timed_out {
            FetchError::TimedOut
        } else {
            FetchError::Failed
        })
    }

    async fn exchange<S>(&self, io: S, host_header: &str, target: &str) -> Result<Reply, FetchError>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let (mut sender, connection) =
            hyper::client::conn::http1::handshake::<_, Empty<Bytes>>(TokioIo::new(io))
                .await
                .map_err(|_| FetchError::Failed)?;
        let driver = tokio::spawn(async move {
            let _ = connection.await;
        });
        // The connection task ends with this fetch whatever happens below.
        let _abort = AbortOnDrop(driver);
        let request = Request::builder()
            .method(Method::GET)
            .uri(target)
            .header(HOST, host_header)
            .header(USER_AGENT, USER_AGENT_VALUE)
            .header(
                ACCEPT,
                "application/rss+xml, application/atom+xml, application/xml;q=0.9, text/xml;q=0.8",
            )
            // No compression: nothing can expand past the size cap.
            .header(ACCEPT_ENCODING, "identity")
            .header(CONNECTION, "close")
            .body(Empty::<Bytes>::new())
            .map_err(|_| FetchError::Refused)?;
        let response = sender
            .send_request(request)
            .await
            .map_err(|_| FetchError::Failed)?;
        let status = response.status();
        if matches!(
            status,
            StatusCode::MOVED_PERMANENTLY
                | StatusCode::FOUND
                | StatusCode::SEE_OTHER
                | StatusCode::TEMPORARY_REDIRECT
                | StatusCode::PERMANENT_REDIRECT
        ) {
            let location = response
                .headers()
                .get(LOCATION)
                .and_then(|v| v.to_str().ok())
                .ok_or(FetchError::Failed)?
                .to_owned();
            return Ok(Reply::Redirect(location));
        }
        if !status.is_success() {
            return Err(FetchError::Failed);
        }
        let max = self.limits.max_bytes;
        let declared = response
            .headers()
            .get(hyper::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok());
        if declared.is_some_and(|n| n > max as u64) {
            return Err(FetchError::TooLarge);
        }
        let mut body = response.into_body();
        let mut out: Vec<u8> = Vec::new();
        while let Some(frame) = body.frame().await {
            let frame = frame.map_err(|_| FetchError::Failed)?;
            if let Some(data) = frame.data_ref() {
                if out.len().saturating_add(data.len()) > max {
                    return Err(FetchError::TooLarge);
                }
                out.extend_from_slice(data);
            }
        }
        Ok(Reply::Body(out))
    }
}

struct AbortOnDrop(tokio::task::JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

enum Reply {
    Body(Vec<u8>),
    Redirect(String),
}

impl<R: Resolve> Fetch for GuardedFetcher<R> {
    async fn get(&self, url: &FeedUrl, allow_http: bool) -> Result<Vec<u8>, FetchError> {
        // The address rules on the URL itself (scheme, credentials) are core's; plain http needs
        // the flag even here, so a caller cannot forget it.
        if !url.is_https() && !allow_http {
            return Err(FetchError::Refused);
        }
        timeout(self.limits.total, self.run(url, allow_http))
            .await
            .map_err(|_| FetchError::TimedOut)?
    }
}
