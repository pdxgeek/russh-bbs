//! Feeds, server side, with no live network: the guard's address table, the fetcher against a
//! local scripted server that serves hostile answers, and the whole read path (fetch, parse,
//! store) against a real SQLite database.
#![allow(clippy::unwrap_used, clippy::panic, clippy::too_many_lines)]

use std::collections::HashMap;
use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use russh_bbs_core::access::{AccessStore, Slug};
use russh_bbs_core::feeds::{self, FeedStore, FeedUrl, Fetch, FetchError, Outcome, poll_due};
use russh_bbs_core::forums::{self, ForumStore};
use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::store::{AccountState, AccountStore, NewAccount, UnixSeconds};
use russh_bbs_store::SqliteStore;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use super::{FetchLimits, Guard, GuardedFetcher, Resolve, USER_AGENT_VALUE, is_public_ip};

// ---- the address table ------------------------------------------------------------------

fn v4(a: u8, b: u8, c: u8, d: u8) -> IpAddr {
    IpAddr::V4(Ipv4Addr::new(a, b, c, d))
}

fn v6(text: &str) -> IpAddr {
    IpAddr::V6(text.parse::<Ipv6Addr>().unwrap())
}

#[test]
fn private_loopback_link_local_metadata_and_special_addresses_are_refused() {
    let refused: Vec<IpAddr> = vec![
        v4(0, 0, 0, 0),
        v4(0, 1, 2, 3),
        v4(10, 0, 0, 1),
        v4(10, 255, 255, 255),
        v4(100, 64, 0, 1),
        v4(100, 100, 100, 200),
        v4(100, 127, 255, 255),
        v4(127, 0, 0, 1),
        v4(127, 255, 255, 254),
        v4(169, 254, 0, 1),
        v4(169, 254, 169, 254),
        v4(172, 16, 0, 1),
        v4(172, 31, 255, 255),
        v4(192, 0, 0, 1),
        v4(192, 0, 2, 1),
        v4(192, 88, 99, 1),
        v4(192, 168, 0, 1),
        v4(192, 168, 255, 255),
        v4(198, 18, 0, 1),
        v4(198, 19, 255, 255),
        v4(198, 51, 100, 7),
        v4(203, 0, 113, 7),
        v4(224, 0, 0, 1),
        v4(239, 255, 255, 250),
        v4(240, 0, 0, 1),
        v4(255, 255, 255, 255),
        v6("::"),
        v6("::1"),
        v6("fe80::1"),
        v6("febf::1"),
        v6("fec0::1"),
        v6("fc00::1"),
        v6("fd00:ec2::254"),
        v6("fdff:ffff::1"),
        v6("ff02::1"),
        v6("ff0e::1"),
        v6("2001:db8::1"),
        v6("2001::1"),
        v6("2001:2::1"),
        v6("2001:10::1"),
        v6("3fff::1"),
        v6("100::1"),
        v6("64:ff9b:1::1"),
        // IPv4-mapped, IPv4-compatible, NAT64 and 6to4 forms of the refused v4 addresses.
        v6("::ffff:127.0.0.1"),
        v6("::ffff:10.1.2.3"),
        v6("::ffff:192.168.0.1"),
        v6("::ffff:169.254.169.254"),
        v6("::ffff:100.64.0.1"),
        v6("::ffff:0.0.0.0"),
        v6("::ffff:224.0.0.1"),
        v6("::127.0.0.1"),
        v6("::10.0.0.1"),
        v6("64:ff9b::7f00:1"),
        v6("64:ff9b::a9fe:a9fe"),
        v6("2002:7f00:1::1"),
        v6("2002:a9fe:a9fe::1"),
        v6("2002:c0a8:1::1"),
    ];
    for ip in refused {
        assert!(!is_public_ip(ip), "{ip} must be refused");
    }
}

#[test]
fn ordinary_public_addresses_are_allowed() {
    let allowed: Vec<IpAddr> = vec![
        v4(1, 1, 1, 1),
        v4(8, 8, 8, 8),
        v4(93, 184, 216, 34),
        v4(100, 63, 255, 255),
        v4(100, 128, 0, 1),
        v4(172, 15, 255, 255),
        v4(172, 32, 0, 1),
        v4(169, 253, 1, 1),
        v4(169, 255, 1, 1),
        v4(192, 167, 1, 1),
        v4(192, 169, 1, 1),
        v4(198, 17, 1, 1),
        v4(198, 20, 1, 1),
        v4(223, 255, 255, 255),
        v6("2606:4700:4700::1111"),
        v6("2a00:1450:4001::200e"),
        v6("2001:4860:4860::8888"),
        v6("::ffff:8.8.8.8"),
        v6("64:ff9b::808:808"),
        v6("2002:808:808::1"),
    ];
    for ip in allowed {
        assert!(is_public_ip(ip), "{ip} must be allowed");
    }
}

#[test]
fn the_guard_refuses_everything_the_standard_library_calls_special() {
    // Every /24 of IPv4: nothing std considers private, loopback, link-local, multicast,
    // unspecified, broadcast or documentation may pass. (std knows fewer ranges than the guard,
    // so this is a floor, not the whole table.)
    for a in 0..=255_u8 {
        for b in 0..=255_u8 {
            for c in 0..=255_u8 {
                let ip = Ipv4Addr::new(a, b, c, 1);
                let special = ip.is_private()
                    || ip.is_loopback()
                    || ip.is_link_local()
                    || ip.is_multicast()
                    || ip.is_unspecified()
                    || ip.is_broadcast()
                    || ip.is_documentation();
                if special {
                    assert!(!is_public_ip(IpAddr::V4(ip)), "{ip}");
                }
                // An IPv4-mapped IPv6 address is judged by the address inside it.
                if (a, b) == (10, 0) || (a, b) == (127, 0) {
                    assert!(!is_public_ip(IpAddr::V6(ip.to_ipv6_mapped())), "{ip}");
                }
            }
        }
    }
}

// ---- a scripted local server ------------------------------------------------------------

#[derive(Debug, Clone)]
struct Seen {
    path: String,
    headers: HashMap<String, String>,
}

enum Reply {
    /// A complete response, sent as is.
    Raw(Vec<u8>),
    /// A 200 with this body and a `Content-Length`.
    Ok(Vec<u8>),
    /// A redirect to this `Location`.
    Redirect(String),
    /// A 200 that never ends: the head, then this many bytes every interval, forever.
    Drip { every: Duration, chunk: usize },
    /// A 200 with chunked encoding and an endless body.
    Endless,
    /// Accept the connection and send nothing.
    Silent,
}

type Handler = dyn Fn(&Seen) -> Reply + Send + Sync;

struct Server {
    addr: SocketAddr,
    seen: Arc<Mutex<Vec<Seen>>>,
}

impl Server {
    async fn start(handler: impl Fn(&Seen) -> Reply + Send + Sync + 'static) -> Self {
        Self::start_with(None, handler).await
    }

    /// With a TLS acceptor, the server speaks https.
    async fn start_with(
        tls: Option<tokio_rustls::TlsAcceptor>,
        handler: impl Fn(&Seen) -> Reply + Send + Sync + 'static,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let handler: Arc<Handler> = Arc::new(handler);
        let log = Arc::clone(&seen);
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                let handler = Arc::clone(&handler);
                let log = Arc::clone(&log);
                let tls = tls.clone();
                tokio::spawn(async move {
                    match tls {
                        Some(acceptor) => {
                            if let Ok(stream) = acceptor.accept(stream).await {
                                let _ = serve_one(stream, &*handler, &log).await;
                            }
                        }
                        None => {
                            let _ = serve_one(stream, &*handler, &log).await;
                        }
                    }
                });
            }
        });
        Self { addr, seen }
    }

    fn requests(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }

    fn hits(&self) -> usize {
        self.requests().len()
    }
}

async fn serve_one<S: AsyncRead + AsyncWrite + Unpin>(
    mut stream: S,
    handler: &Handler,
    log: &Mutex<Vec<Seen>>,
) -> io::Result<()> {
    let mut head = Vec::new();
    let mut byte = [0_u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if stream.read(&mut byte).await? == 0 || head.len() > 16_384 {
            return Ok(());
        }
        head.push(byte[0]);
    }
    let text = String::from_utf8_lossy(&head).into_owned();
    let mut lines = text.split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    let path = request_line
        .split(' ')
        .nth(1)
        .unwrap_or_default()
        .to_owned();
    let headers = lines
        .filter_map(|l| l.split_once(": "))
        .map(|(k, v)| (k.to_ascii_lowercase(), v.to_owned()))
        .collect();
    let seen = Seen { path, headers };
    log.lock().unwrap().push(seen.clone());
    match handler(&seen) {
        Reply::Raw(bytes) => stream.write_all(&bytes).await?,
        Reply::Ok(body) => {
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/rss+xml\r\nContent-Length: {}\r\n\
                 Connection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(head.as_bytes()).await?;
            stream.write_all(&body).await?;
        }
        Reply::Redirect(location) => {
            let head = format!(
                "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\n\
                 Connection: close\r\n\r\n"
            );
            stream.write_all(head.as_bytes()).await?;
        }
        Reply::Drip { every, chunk } => {
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n")
                .await?;
            loop {
                tokio::time::sleep(every).await;
                stream.write_all(&vec![b'a'; chunk]).await?;
            }
        }
        Reply::Endless => {
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
                )
                .await?;
            let chunk = vec![b'x'; 16_384];
            loop {
                stream
                    .write_all(format!("{:x}\r\n", chunk.len()).as_bytes())
                    .await?;
                stream.write_all(&chunk).await?;
                stream.write_all(b"\r\n").await?;
            }
        }
        Reply::Silent => {
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    }
    Ok(())
}

/// A resolver that answers from a table and counts the questions it was asked.
#[derive(Clone, Default)]
struct Table {
    names: Arc<Mutex<HashMap<String, Vec<Vec<IpAddr>>>>>,
    asked: Arc<AtomicUsize>,
}

impl Table {
    fn with(name: &str, answers: Vec<Vec<IpAddr>>) -> Self {
        let table = Self::default();
        table.names.lock().unwrap().insert(name.to_owned(), answers);
        table
    }

    fn and(self, name: &str, answers: Vec<Vec<IpAddr>>) -> Self {
        self.names.lock().unwrap().insert(name.to_owned(), answers);
        self
    }

    fn asked(&self) -> usize {
        self.asked.load(Ordering::SeqCst)
    }
}

#[allow(clippy::unused_async_trait_impl)]
impl Resolve for Table {
    async fn resolve(&self, host: &str, port: u16) -> io::Result<Vec<SocketAddr>> {
        let n = self.asked.fetch_add(1, Ordering::SeqCst);
        let names = self.names.lock().unwrap();
        let answers = names
            .get(host)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no such name"))?;
        // The Nth question gets the Nth answer, the last answer repeating.
        let answer = answers.get(n.min(answers.len() - 1)).unwrap();
        // `asked` counts every name; the per-name index above is only right for one name, which
        // is how these tests use it.
        Ok(answer.iter().map(|ip| SocketAddr::new(*ip, port)).collect())
    }
}

fn limits() -> FetchLimits {
    FetchLimits {
        connect: Duration::from_millis(800),
        total: Duration::from_millis(1_500),
        max_bytes: 64 * 1024,
        max_redirects: 3,
    }
}

const LOOPBACK: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

/// A fetcher that resolves `feed.test` to loopback and lets exactly the given server through.
fn fetcher_for(server: &Server, extra: Table) -> GuardedFetcher<Table> {
    let table = extra.and("feed.test", vec![vec![LOOPBACK]]);
    GuardedFetcher::with_resolver(table, Guard::new().allowing([server.addr]), limits())
}

fn url_of(server: &Server, path: &str) -> FeedUrl {
    FeedUrl::parse(
        &format!("http://feed.test:{}{path}", server.addr.port()),
        true,
    )
    .unwrap()
}

const OK_FEED: &str = "<?xml version=\"1.0\"?><rss version=\"2.0\"><channel><title>t</title>\
    <item><title>One</title><guid>1</guid><description>hello quokka</description></item>\
    </channel></rss>";

// ---- the fetcher ---------------------------------------------------------------------------

#[tokio::test]
async fn a_good_feed_is_fetched_with_a_fixed_agent_and_no_cookies_or_compression() {
    let server = Server::start(|_| Reply::Ok(OK_FEED.as_bytes().to_vec())).await;
    let fetcher = fetcher_for(&server, Table::default());
    let body = fetcher
        .get(&url_of(&server, "/rss?a=1&b=2"), true)
        .await
        .unwrap();
    assert_eq!(body, OK_FEED.as_bytes());
    let seen = server.requests();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].path, "/rss?a=1&b=2");
    let h = &seen[0].headers;
    assert_eq!(h["user-agent"], USER_AGENT_VALUE);
    assert!(USER_AGENT_VALUE.starts_with("russh-bbs-feed-fetcher/"));
    assert_eq!(h["host"], format!("feed.test:{}", server.addr.port()));
    assert_eq!(h["accept-encoding"], "identity");
    assert_eq!(h["connection"], "close");
    for forbidden in ["cookie", "authorization", "referer", "proxy-authorization"] {
        assert!(!h.contains_key(forbidden), "{forbidden} must not be sent");
    }
    // A Set-Cookie from the server is not kept or sent back on the next fetch.
    let server = Server::start(|seen| {
        if seen.headers.contains_key("cookie") {
            Reply::Raw(b"HTTP/1.1 500 Cookie\r\nContent-Length: 0\r\n\r\n".to_vec())
        } else {
            Reply::Raw(
                format!(
                    "HTTP/1.1 200 OK\r\nSet-Cookie: s=1\r\nContent-Length: {}\r\n\r\n{OK_FEED}",
                    OK_FEED.len()
                )
                .into_bytes(),
            )
        }
    })
    .await;
    let fetcher = fetcher_for(&server, Table::default());
    for _ in 0..2 {
        assert!(fetcher.get(&url_of(&server, "/"), true).await.is_ok());
    }
}

#[tokio::test]
async fn plain_http_needs_the_per_feed_flag_even_for_a_caller_that_forgets() {
    let server = Server::start(|_| Reply::Ok(OK_FEED.as_bytes().to_vec())).await;
    let fetcher = fetcher_for(&server, Table::default());
    assert_eq!(
        fetcher.get(&url_of(&server, "/"), false).await,
        Err(FetchError::Refused)
    );
    assert_eq!(server.hits(), 0, "nothing was connected to");
}

#[tokio::test]
async fn a_redirect_to_loopback_or_metadata_or_any_private_address_is_refused_before_connecting() {
    let secret = Server::start(|_| Reply::Ok(b"SECRET".to_vec())).await;
    let port = secret.addr.port();
    let targets = [
        format!("http://127.0.0.1:{port}/secret"),
        format!("http://[::1]:{port}/secret"),
        format!("http://[::ffff:127.0.0.1]:{port}/secret"),
        format!("http://[::ffff:7f00:1]:{port}/secret"),
        format!("http://2130706433:{port}/secret"),
        format!("http://0x7f.1:{port}/secret"),
        format!("http://0177.0.0.1:{port}/secret"),
        format!("http://127.1:{port}/secret"),
        format!("http://localhost:{port}/secret"),
        "http://169.254.169.254/latest/meta-data/iam/security-credentials/".to_owned(),
        "http://[fd00:ec2::254]/latest/meta-data/".to_owned(),
        "http://10.0.0.1/".to_owned(),
        "http://172.16.5.5/".to_owned(),
        "http://192.168.1.1/admin".to_owned(),
        "http://100.64.0.1/".to_owned(),
        "http://0.0.0.0/".to_owned(),
        "http://[::]/".to_owned(),
        "//127.0.0.1/x".to_owned(),
    ];
    for target in targets {
        let to = target.clone();
        let first = Server::start(move |_| Reply::Redirect(to.clone())).await;
        let table = Table::with("localhost", vec![vec![LOOPBACK]]);
        let fetcher = fetcher_for(&first, table);
        assert_eq!(
            fetcher.get(&url_of(&first, "/start"), true).await,
            Err(FetchError::Refused),
            "{target}"
        );
        assert_eq!(first.hits(), 1, "the first hop was fetched once: {target}");
    }
    assert_eq!(secret.hits(), 0, "no redirect reached the loopback server");
}

#[tokio::test]
async fn a_name_that_resolves_to_a_private_address_is_refused_and_never_connected_to() {
    let victim = Server::start(|_| Reply::Ok(b"SECRET".to_vec())).await;
    for private in [
        v4(10, 0, 0, 5),
        v4(127, 0, 0, 1),
        v4(169, 254, 169, 254),
        v6("::1"),
        v6("fd00:ec2::254"),
        v6("::ffff:127.0.0.1"),
    ] {
        let table = Table::with("evil.test", vec![vec![private]]);
        let fetcher = GuardedFetcher::with_resolver(table.clone(), Guard::new(), limits());
        let url =
            FeedUrl::parse(&format!("http://evil.test:{}/", victim.addr.port()), true).unwrap();
        assert_eq!(
            fetcher.get(&url, true).await,
            Err(FetchError::Refused),
            "{private}"
        );
        assert_eq!(table.asked(), 1);
    }
    // One private answer among public ones refuses the whole name.
    let table = Table::with("mixed.test", vec![vec![v4(8, 8, 8, 8), v4(10, 0, 0, 1)]]);
    let fetcher = GuardedFetcher::with_resolver(table, Guard::new(), limits());
    let url = FeedUrl::parse("https://mixed.test/feed", false).unwrap();
    assert_eq!(fetcher.get(&url, false).await, Err(FetchError::Refused));
    assert_eq!(victim.hits(), 0);
}

#[tokio::test]
async fn dns_rebinding_cannot_swap_the_address_after_the_check() {
    // The name answers with the (allowed) server the first time and with loopback-private
    // addresses after. One lookup per hop, and the connection goes to the checked answer.
    let server = Server::start(|_| Reply::Ok(OK_FEED.as_bytes().to_vec())).await;
    let table = Table::with(
        "rebind.test",
        vec![vec![LOOPBACK], vec![v4(10, 0, 0, 1)], vec![v4(10, 0, 0, 1)]],
    );
    let fetcher = GuardedFetcher::with_resolver(
        table.clone(),
        Guard::new().allowing([server.addr]),
        limits(),
    );
    let url = FeedUrl::parse(&format!("http://rebind.test:{}/", server.addr.port()), true).unwrap();
    assert_eq!(fetcher.get(&url, true).await.unwrap(), OK_FEED.as_bytes());
    assert_eq!(table.asked(), 1, "exactly one lookup for the one hop");
    assert_eq!(server.hits(), 1);
    // A redirect back to the same name does a second lookup, which now answers privately: refused.
    let path = format!("http://rebind.test:{}/again", server.addr.port());
    let redirecting = Server::start(move |_| Reply::Redirect(path.clone())).await;
    let table = Table::with("rebind.test", vec![vec![LOOPBACK], vec![v4(10, 0, 0, 1)]]);
    let fetcher = GuardedFetcher::with_resolver(
        table.clone(),
        Guard::new().allowing([redirecting.addr]),
        limits(),
    );
    let url = FeedUrl::parse(
        &format!("http://rebind.test:{}/", redirecting.addr.port()),
        true,
    )
    .unwrap();
    assert_eq!(fetcher.get(&url, true).await, Err(FetchError::Refused));
    assert_eq!(table.asked(), 2);
    assert_eq!(redirecting.hits(), 1);
}

#[tokio::test]
async fn redirects_are_capped_and_bad_locations_are_refused() {
    // A loop: every answer redirects to the same path.
    let server = Server::start(|_| Reply::Redirect("/again".into())).await;
    let fetcher = fetcher_for(&server, Table::default());
    assert_eq!(
        fetcher.get(&url_of(&server, "/"), true).await,
        Err(FetchError::Refused)
    );
    assert_eq!(
        server.hits(),
        4,
        "the first request plus three redirects, no more"
    );
    // A relative redirect that is fine.
    let server = Server::start(|seen| {
        if seen.path == "/final" {
            Reply::Ok(OK_FEED.as_bytes().to_vec())
        } else {
            Reply::Redirect("/final".into())
        }
    })
    .await;
    let fetcher = fetcher_for(&server, Table::default());
    assert!(fetcher.get(&url_of(&server, "/start"), true).await.is_ok());
    assert_eq!(server.hits(), 2);
    // Locations the URL rules refuse.
    for bad in [
        "ftp://example.org/x",
        "file:///etc/passwd",
        "javascript:alert(1)",
        "http://user:pw@example.org/",
        "http://exa mple.org/",
    ] {
        let to = bad.to_owned();
        let server = Server::start(move |_| Reply::Redirect(to.clone())).await;
        let fetcher = fetcher_for(&server, Table::default());
        assert_eq!(
            fetcher.get(&url_of(&server, "/"), true).await,
            Err(FetchError::Refused),
            "{bad}"
        );
    }
    // A redirect with no Location, and a plain error status, are failures.
    let server =
        Server::start(|_| Reply::Raw(b"HTTP/1.1 302 Found\r\nContent-Length: 0\r\n\r\n".to_vec()))
            .await;
    assert_eq!(
        fetcher_for(&server, Table::default())
            .get(&url_of(&server, "/"), true)
            .await,
        Err(FetchError::Failed)
    );
    for status in [
        "404 Not Found",
        "500 Oops",
        "403 Forbidden",
        "204 No Content",
    ] {
        let line = format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\n\r\n");
        let server = Server::start(move |_| Reply::Raw(line.clone().into_bytes())).await;
        let got = fetcher_for(&server, Table::default())
            .get(&url_of(&server, "/"), true)
            .await;
        // 204 is a success status with an empty body; the parser, not the fetcher, refuses it.
        if status.starts_with("204") {
            assert_eq!(got, Ok(Vec::new()));
        } else {
            assert_eq!(got, Err(FetchError::Failed), "{status}");
        }
    }
}

#[tokio::test]
async fn a_huge_answer_is_stopped_at_the_cap_declared_or_streamed() {
    // Declared up front: refused without reading it.
    let server = Server::start(|_| {
        Reply::Raw(
            b"HTTP/1.1 200 OK\r\nContent-Length: 104857600\r\nConnection: close\r\n\r\nxxxx"
                .to_vec(),
        )
    })
    .await;
    assert_eq!(
        fetcher_for(&server, Table::default())
            .get(&url_of(&server, "/"), true)
            .await,
        Err(FetchError::TooLarge)
    );
    // Chunked with no end: stopped once the cap is crossed, quickly.
    let server = Server::start(|_| Reply::Endless).await;
    let started = Instant::now();
    assert_eq!(
        fetcher_for(&server, Table::default())
            .get(&url_of(&server, "/"), true)
            .await,
        Err(FetchError::TooLarge)
    );
    assert!(started.elapsed() < Duration::from_secs(1));
    // Exactly at the cap is fine; one byte over is not.
    let at_cap = vec![b' '; 64 * 1024];
    let server = Server::start(move |_| Reply::Ok(at_cap.clone())).await;
    assert_eq!(
        fetcher_for(&server, Table::default())
            .get(&url_of(&server, "/"), true)
            .await
            .unwrap()
            .len(),
        64 * 1024
    );
    let over = vec![b' '; 64 * 1024 + 1];
    let server = Server::start(move |_| Reply::Ok(over.clone())).await;
    assert_eq!(
        fetcher_for(&server, Table::default())
            .get(&url_of(&server, "/"), true)
            .await,
        Err(FetchError::TooLarge)
    );
}

#[tokio::test]
async fn a_slow_drip_and_a_silent_server_run_out_of_time() {
    // The head arrives, then a byte at a time, forever.
    let server = Server::start(|_| Reply::Drip {
        every: Duration::from_millis(50),
        chunk: 1,
    })
    .await;
    let started = Instant::now();
    assert_eq!(
        fetcher_for(&server, Table::default())
            .get(&url_of(&server, "/"), true)
            .await,
        Err(FetchError::TimedOut)
    );
    let took = started.elapsed();
    assert!(
        took >= Duration::from_millis(1_400) && took < Duration::from_secs(3),
        "{took:?}"
    );
    // Nothing is ever sent.
    let server = Server::start(|_| Reply::Silent).await;
    assert_eq!(
        fetcher_for(&server, Table::default())
            .get(&url_of(&server, "/"), true)
            .await,
        Err(FetchError::TimedOut)
    );
    // A connection to a closed port is a failure, not a hang.
    let closed = {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        listener.local_addr().unwrap()
    };
    let table = Table::with("feed.test", vec![vec![LOOPBACK]]);
    let fetcher = GuardedFetcher::with_resolver(table, Guard::new().allowing([closed]), limits());
    let url = FeedUrl::parse(&format!("http://feed.test:{}/", closed.port()), true).unwrap();
    assert_eq!(fetcher.get(&url, true).await, Err(FetchError::Failed));
}

#[tokio::test]
async fn a_name_that_does_not_resolve_is_a_failure() {
    let fetcher = GuardedFetcher::with_resolver(Table::default(), Guard::new(), limits());
    let url = FeedUrl::parse("https://nowhere.test/feed", false).unwrap();
    assert_eq!(fetcher.get(&url, false).await, Err(FetchError::Failed));
    let empty = Table::with("empty.test", vec![vec![]]);
    let fetcher = GuardedFetcher::with_resolver(empty, Guard::new(), limits());
    let url = FeedUrl::parse("https://empty.test/feed", false).unwrap();
    assert_eq!(fetcher.get(&url, false).await, Err(FetchError::Failed));
}

// ---- the whole read path ----------------------------------------------------------------

struct Board {
    store: SqliteStore,
    _dir: tempfile::TempDir,
    sysop: russh_bbs_core::access::Snapshot,
    member: russh_bbs_core::access::Snapshot,
}

async fn board() -> Board {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(&dir.path().join("bbs.db")).await.unwrap();
    let mut accounts = Vec::new();
    for (name, role) in [("ops", Slug::SYSOP), ("ann", Slug::MEMBER)] {
        accounts.push(
            store
                .create_account(NewAccount {
                    username: sanitize_username(name).unwrap(),
                    role,
                    state: AccountState::Active,
                })
                .await
                .unwrap(),
        );
    }
    let sysop = store.snapshot(Some(accounts[0].id)).await.unwrap();
    let member = store.snapshot(Some(accounts[1].id)).await.unwrap();
    Board {
        store,
        _dir: dir,
        sysop,
        member,
    }
}

fn now(n: i64) -> UnixSeconds {
    UnixSeconds::from_raw(1_800_000_000 + n)
}

fn evil_feed() -> String {
    let evil = "&#x1b;[31mRED&#x1b;[0m &#x1b;]0;pwned&#x7; &#x9b;31m &#x85; \u{202e}rtl\u{200b} \
                \u{e000}\u{f8ff}\u{27e6}\u{27e7}\u{2299}\u{229b}\u{270e}\u{26a0}\u{feff} ok";
    format!(
        "<?xml version=\"1.0\"?><rss version=\"2.0\"><channel><title>{evil}</title>\
         <item><title>{evil}</title><link>https://example.org/{evil}</link><guid>e1</guid>\
         <author>{evil}</author><description><![CDATA[{evil}<script>x</script>\
         <a href=\"javascript:alert(1)\">x</a>quokka]]></description></item></channel></rss>"
    )
}

#[tokio::test]
async fn every_hostile_server_behaviour_is_neutralised_or_refused_end_to_end() {
    let b = board().await;
    let billion = "<?xml version=\"1.0\"?><!DOCTYPE lolz [<!ENTITY lol \"lol\">\
        <!ENTITY lol2 \"&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;\">\
        <!ENTITY lol3 \"&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;\">]>\
        <rss version=\"2.0\"><channel><title>&lol3;</title></channel></rss>"
        .to_owned();
    let big = format!(
        "<?xml version=\"1.0\"?><rss version=\"2.0\"><channel><title>t</title><item><title>x</title>\
         <guid>big</guid><description>{}</description></item></channel></rss>",
        "word ".repeat(20_000)
    );
    let huge = Arc::new(big);
    let secret = Server::start(|_| Reply::Ok(b"SECRET".to_vec())).await;
    let secret_port = secret.addr.port();
    let to_loopback = format!("http://127.0.0.1:{secret_port}/");
    let evil_doc = Arc::new(evil_feed());
    let server = Server::start(move |seen| match seen.path.as_str() {
        "/good" => Reply::Ok(OK_FEED.as_bytes().to_vec()),
        "/evil" => Reply::Ok(evil_doc.as_bytes().to_vec()),
        "/billion" => Reply::Ok(billion.clone().into_bytes()),
        "/huge" => Reply::Ok(huge.as_bytes().to_vec()),
        "/endless" => Reply::Endless,
        "/to-loopback" => Reply::Redirect(to_loopback.clone()),
        "/to-metadata" => Reply::Redirect("http://169.254.169.254/latest/meta-data/".into()),
        "/drip" => Reply::Drip {
            every: Duration::from_millis(50),
            chunk: 1,
        },
        "/html" => Reply::Ok(b"<html><body>hello</body></html>".to_vec()),
        _ => Reply::Raw(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".to_vec()),
    })
    .await;
    let port = server.addr.port();
    let table = Table::with("feed.test", vec![vec![LOOPBACK]])
        .and("rebinds.test", vec![vec![v4(10, 0, 0, 7)]]);
    let fetcher = GuardedFetcher::with_resolver(
        table,
        Guard::new().allowing([server.addr]),
        FetchLimits {
            max_bytes: 128 * 1024,
            ..limits()
        },
    );

    let cases = [
        ("good", format!("http://feed.test:{port}/good"), Outcome::Ok),
        ("evil", format!("http://feed.test:{port}/evil"), Outcome::Ok),
        (
            "billion",
            format!("http://feed.test:{port}/billion"),
            Outcome::Invalid,
        ),
        ("huge", format!("http://feed.test:{port}/huge"), Outcome::Ok),
        (
            "endless",
            format!("http://feed.test:{port}/endless"),
            Outcome::TooLarge,
        ),
        (
            "loopback",
            format!("http://feed.test:{port}/to-loopback"),
            Outcome::Refused,
        ),
        (
            "metadata",
            format!("http://feed.test:{port}/to-metadata"),
            Outcome::Refused,
        ),
        (
            "drip",
            format!("http://feed.test:{port}/drip"),
            Outcome::TimedOut,
        ),
        (
            "html",
            format!("http://feed.test:{port}/html"),
            Outcome::Invalid,
        ),
        (
            "missing",
            format!("http://feed.test:{port}/missing"),
            Outcome::Failed,
        ),
        (
            "private",
            format!("http://rebinds.test:{port}/good"),
            Outcome::Refused,
        ),
    ];
    for (name, url, _) in &cases {
        feeds::add_feed(&b.store, &b.sysop, now(0), url, name, true, None)
            .await
            .unwrap();
    }
    let report = poll_due(&b.store, &fetcher, now(10), 50).await.unwrap();
    assert_eq!(report.tried, 11);
    let rows = b.store.feeds().await.unwrap();
    for (name, _, expected) in &cases {
        let row = rows.iter().find(|r| r.board_name == *name).unwrap();
        assert_eq!(row.last_outcome, Some(*expected), "{name}");
        assert_eq!(row.failures, u32::from(*expected != Outcome::Ok), "{name}");
    }
    assert_eq!(secret.hits(), 0, "no redirect reached the loopback server");

    // Only the readable ones put anything on a board, and what they put there is clean.
    let all = forums::boards(&b.store, &b.member).await.unwrap();
    for board in &all {
        let page = forums::threads(&b.store, &b.member, board.id, 0)
            .await
            .unwrap();
        let expect_items = matches!(board.name.as_str(), "good" | "evil" | "huge");
        assert_eq!(!page.threads.is_empty(), expect_items, "{}", board.name);
        for thread in &page.threads {
            let view = forums::thread_post(&b.store, &b.member, thread.id, 0)
                .await
                .unwrap();
            for text in [thread.subject.as_str(), view.post.body.as_str()] {
                for c in text.chars() {
                    assert!(
                        (!c.is_control() || c == '\n' || c == '\t')
                            && !russh_bbs_core::sanitize::RESERVED_CHROME.contains(&c)
                            && !('\u{E000}'..='\u{F8FF}').contains(&c)
                            && !matches!(c, '\u{202e}' | '\u{200b}' | '\u{feff}'),
                        "{c:?} in {text:?} on {}",
                        board.name
                    );
                }
                assert!(!text.contains("<script") && !text.contains("javascript:"));
            }
            if board.name == "huge" {
                assert!(view.post.body.as_str().chars().count() <= 8_000);
            }
        }
    }
    // The evil title keeps its harmless words.
    let evil = all.iter().find(|b| b.name == "evil").unwrap();
    let page = forums::threads(&b.store, &b.member, evil.id, 0)
        .await
        .unwrap();
    assert!(page.threads[0].subject.contains("RED") && page.threads[0].subject.contains("ok"));

    // A failing feed backs off: not due again right away, due after its (doubled) wait.
    let again = poll_due(&b.store, &fetcher, now(20), 50).await.unwrap();
    assert_eq!(
        again.tried, 0,
        "healthy feeds wait their interval and failing ones back off"
    );
    let later = poll_due(&b.store, &fetcher, now(3_600 + 100), 50)
        .await
        .unwrap();
    assert_eq!(
        later.tried, 3,
        "the three that worked are due after an hour"
    );
    let much_later = poll_due(&b.store, &fetcher, now(2 * 3_600 + 200), 50)
        .await
        .unwrap();
    assert_eq!(much_later.tried, 11 - 3 + 3);
    // Re-reading stored nothing twice.
    let page = forums::threads(&b.store, &b.member, all[0].id, 0)
        .await
        .unwrap();
    assert_eq!(page.threads.len(), 1);
}

#[tokio::test]
async fn a_feed_board_can_be_read_and_searched_and_cannot_be_written_to_over_the_fetch_path() {
    let b = board().await;
    let server = Server::start(|_| Reply::Ok(OK_FEED.as_bytes().to_vec())).await;
    let fetcher = fetcher_for(&server, Table::default());
    let url = format!("http://feed.test:{}/rss", server.addr.port());
    let added = feeds::add_feed(&b.store, &b.sysop, now(0), &url, "News", true, None)
        .await
        .unwrap();
    assert_eq!(
        poll_due(&b.store, &fetcher, now(5), 10)
            .await
            .unwrap()
            .added,
        1
    );
    let page = forums::threads(&b.store, &b.member, added.board, 0)
        .await
        .unwrap();
    assert_eq!(page.threads.len(), 1);
    assert!(page.board.feed && !page.can_post);
    let found = forums::search(&b.store, &b.member, "quokka", 0)
        .await
        .unwrap();
    assert_eq!(found.hits.len(), 1);
    let body = russh_bbs_core::drafts::Body::prepare(forums::body_limits(), "hi").unwrap();
    assert!(matches!(
        forums::post_thread(&b.store, &b.member, now(6), added.board, "x", &body).await,
        Err(forums::ForumError::ReadOnly)
    ));
    assert!(matches!(
        forums::post_reply(&b.store, &b.sysop, now(6), page.threads[0].id, &body).await,
        Err(forums::ForumError::ReadOnly)
    ));
    assert_eq!(b.store.boards().await.unwrap().len(), 1);
}

// ---- https ---------------------------------------------------------------------------------

/// A self-signed certificate for `name`, made now: the server's acceptor and the certificate a
/// fetcher can be told to trust.
fn tls_for(
    name: &str,
) -> (
    tokio_rustls::TlsAcceptor,
    rustls_pki_types::CertificateDer<'static>,
) {
    let made = rcgen::generate_simple_self_signed(vec![name.to_owned()]).unwrap();
    let cert = made.cert.der().clone();
    let key = rustls_pki_types::PrivateKeyDer::try_from(made.signing_key.serialize_der()).unwrap();
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(vec![cert.clone()], key)
    .unwrap();
    (tokio_rustls::TlsAcceptor::from(Arc::new(config)), cert)
}

fn https_url(host: &str, server: &Server, path: &str) -> FeedUrl {
    FeedUrl::parse(
        &format!("https://{host}:{}{path}", server.addr.port()),
        false,
    )
    .unwrap()
}

#[tokio::test]
async fn https_is_verified_and_a_certificate_nobody_trusts_is_refused() {
    let (acceptor, cert) = tls_for("secure.test");
    let server =
        Server::start_with(Some(acceptor), |_| Reply::Ok(OK_FEED.as_bytes().to_vec())).await;
    let table =
        Table::with("secure.test", vec![vec![LOOPBACK]]).and("other.test", vec![vec![LOOPBACK]]);
    let guard = Guard::new().allowing([server.addr]);
    // Trusting the certificate: fetched, over TLS, with the same request headers.
    let trusting =
        GuardedFetcher::with_resolver(table.clone(), guard.clone(), limits()).trusting(vec![cert]);
    let body = trusting
        .get(&https_url("secure.test", &server, "/rss"), false)
        .await
        .unwrap();
    assert_eq!(body, OK_FEED.as_bytes());
    assert_eq!(server.requests()[0].headers["user-agent"], USER_AGENT_VALUE);
    // The certificate is for another name: refused.
    assert_eq!(
        trusting
            .get(&https_url("other.test", &server, "/rss"), false)
            .await,
        Err(FetchError::Failed)
    );
    // Not trusted at all (the platform roots do not know it): refused.
    let plain = GuardedFetcher::with_resolver(table, guard, limits());
    assert_eq!(
        plain
            .get(&https_url("secure.test", &server, "/rss"), false)
            .await,
        Err(FetchError::Failed)
    );
    // Only the trusted fetch got as far as a request.
    assert_eq!(server.hits(), 1);
}

#[tokio::test]
async fn a_redirect_may_go_up_to_https_but_never_down_to_http() {
    let (acceptor, cert) = tls_for("secure.test");
    let secure =
        Server::start_with(Some(acceptor), |_| Reply::Ok(OK_FEED.as_bytes().to_vec())).await;
    let plain = Server::start(|_| Reply::Ok(b"PLAIN".to_vec())).await;
    let table =
        Table::with("secure.test", vec![vec![LOOPBACK]]).and("feed.test", vec![vec![LOOPBACK]]);
    // http -> https: allowed.
    let up = format!("https://secure.test:{}/rss", secure.addr.port());
    let start = Server::start(move |_| Reply::Redirect(up.clone())).await;
    let guard_up = Guard::new().allowing([secure.addr, start.addr]);
    let fetcher_up = GuardedFetcher::with_resolver(table.clone(), guard_up, limits())
        .trusting(vec![cert.clone()]);
    assert_eq!(
        fetcher_up.get(&url_of(&start, "/"), true).await.unwrap(),
        OK_FEED.as_bytes()
    );
    // https -> http: refused even though this feed allows http, and the http server is never asked.
    let (acceptor, cert2) = tls_for("secure.test");
    let down = format!("http://feed.test:{}/", plain.addr.port());
    let downgrading =
        Server::start_with(Some(acceptor), move |_| Reply::Redirect(down.clone())).await;
    let guard_down = Guard::new().allowing([plain.addr, downgrading.addr]);
    let fetcher_down =
        GuardedFetcher::with_resolver(table, guard_down, limits()).trusting(vec![cert2]);
    assert_eq!(
        fetcher_down
            .get(&https_url("secure.test", &downgrading, "/"), true)
            .await,
        Err(FetchError::Refused)
    );
    assert_eq!(plain.hits(), 0);
}

// ---- RSS out -------------------------------------------------------------------------------

use super::{RssOut, RssOutConfig};

async fn get(addr: SocketAddr, request: &str) -> String {
    let mut stream = TcpStream::connect(addr).await.unwrap();
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut out = Vec::new();
    tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut out))
        .await
        .unwrap()
        .unwrap();
    String::from_utf8_lossy(&out).into_owned()
}

/// The response with the per-response `date` header removed, for comparing two of them.
fn undated(response: &str) -> String {
    response
        .split("\r\n")
        .filter(|l| !l.to_ascii_lowercase().starts_with("date:"))
        .collect::<Vec<_>>()
        .join("\r\n")
}

fn body_of(response: &str) -> &str {
    response.split_once("\r\n\r\n").map_or("", |(_, b)| b)
}

#[tokio::test]
async fn rss_out_serves_only_public_open_boards_and_answers_everything_else_alike() {
    let b = board().await;
    let news = feeds::add_feed(
        &b.store,
        &b.sysop,
        now(0),
        "https://example.org/rss",
        "News",
        false,
        None,
    )
    .await
    .unwrap();
    let hidden = feeds::add_feed(
        &b.store,
        &b.sysop,
        now(0),
        "https://example.org/hidden",
        "Hidden",
        false,
        None,
    )
    .await
    .unwrap();
    let staff = forums::create_board(&b.store, &b.sysop, now(0), "Staff", "", true)
        .await
        .unwrap();
    let xml = rss_with_hostile_text();
    let parsed = feeds::parse(xml.as_bytes(), now(5)).unwrap();
    for feed in b.store.feeds().await.unwrap() {
        b.store
            .ingest(feed.id, &parsed.items, now(5), 200)
            .await
            .unwrap()
            .unwrap();
    }
    feeds::publish_board(&b.store, &b.sysop, now(6), news.board, true)
        .await
        .unwrap();

    let store = Arc::new(b.store.clone());
    let out = RssOut::start(
        &RssOutConfig {
            listen: "127.0.0.1:0".parse().unwrap(),
            base_url: "https://bbs.example/feeds".into(),
        },
        store,
    )
    .await
    .unwrap();
    let addr = out.local_addr();

    let ok = get(
        addr,
        &format!(
            "GET /rss/{} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
            news.board
        ),
    )
    .await;
    assert!(ok.starts_with("HTTP/1.1 200"), "{ok}");
    let lower = ok.to_ascii_lowercase();
    assert!(lower.contains("content-type: application/rss+xml; charset=utf-8"));
    assert!(lower.contains("x-content-type-options: nosniff"));
    let body = body_of(&ok);
    assert!(body.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
    // Valid RSS 2.0 that a feed parser reads back, with the items bounded and the text clean.
    let back = feeds::parse(body.as_bytes(), now(60)).unwrap();
    assert_eq!(back.title.as_str(), "News");
    assert_eq!(back.items.len(), 1);
    for c in body.chars() {
        assert!(!c.is_control() || matches!(c, '\n' | '\t' | '\r'), "{c:?}");
        assert!(!('\u{E000}'..='\u{F8FF}').contains(&c));
        assert!(!russh_bbs_core::sanitize::RESERVED_CHROME.contains(&c));
    }
    assert!(!body.contains("<script"), "{body}");

    // Not public, restricted, unknown, malformed paths: the very same answer.
    let miss = |path: String| async move {
        get(
            addr,
            &format!("GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n"),
        )
        .await
    };
    let reference = undated(&miss(format!("/rss/{}", hidden.board)).await);
    assert!(reference.starts_with("HTTP/1.1 404"), "{reference}");
    for path in [
        format!("/rss/{staff}"),
        "/rss/9999".to_owned(),
        "/rss/0".to_owned(),
        "/rss/-1".to_owned(),
        "/rss/".to_owned(),
        "/rss/1x".to_owned(),
        format!("/rss/{}/", news.board),
        format!("/rss/{}/extra", news.board),
        "/rss/../etc/passwd".to_owned(),
        "/rss/99999999999999999999999".to_owned(),
        "/".to_owned(),
        "/metrics".to_owned(),
        format!("/rss/{}%00", news.board),
    ] {
        assert_eq!(undated(&miss(path.clone()).await), reference, "{path}");
    }
    // Only GET.
    for method in ["POST", "PUT", "DELETE", "HEAD"] {
        let r = get(
            addr,
            &format!("{method} /rss/{} HTTP/1.1\r\nHost: x\r\nContent-Length: 0\r\nConnection: close\r\n\r\n", news.board),
        )
        .await;
        assert!(r.starts_with("HTTP/1.1 405"), "{method}: {r}");
        assert!(r.to_ascii_lowercase().contains("allow: get"));
    }
    // Garbage on the wire closes the connection without a crash, and the listener goes on.
    let _ = get(addr, "\u{0}\u{1}garbage\r\n\r\n").await;
    let again = get(
        addr,
        &format!(
            "GET /rss/{} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
            news.board
        ),
    )
    .await;
    assert!(again.starts_with("HTTP/1.1 200"));
    out.shutdown().await;
    assert!(
        TcpStream::connect(addr).await.is_err(),
        "the port is closed after shutdown"
    );
}

fn rss_with_hostile_text() -> String {
    "<?xml version=\"1.0\"?><rss version=\"2.0\"><channel><title>t</title><item>\
     <title>Hello &#x1b;[31mred &#x27e6;marker&#x27e7; &lt;b&gt;</title><guid>a</guid>\
     <description><![CDATA[<script>alert(1)</script>Body & more <b>text</b> \u{e000}]]></description>\
     </item></channel></rss>"
        .to_owned()
}

#[tokio::test]
async fn a_board_made_private_again_stops_being_served_within_the_cache_time() {
    let b = board().await;
    let news = feeds::add_feed(
        &b.store,
        &b.sysop,
        now(0),
        "https://example.org/rss",
        "News",
        false,
        None,
    )
    .await
    .unwrap();
    // Read straight from the core renderer, as the listener does, to see the mark take effect.
    feeds::publish_board(&b.store, &b.sysop, now(1), news.board, true)
        .await
        .unwrap();
    let anon = feeds::out::public_snapshot(&b.store).await.unwrap();
    assert!(
        feeds::out::render_board(&b.store, &anon, news.board, "https://x", now(2))
            .await
            .unwrap()
            .is_some()
    );
    feeds::publish_board(&b.store, &b.sysop, now(3), news.board, false)
        .await
        .unwrap();
    let anon = feeds::out::public_snapshot(&b.store).await.unwrap();
    assert!(
        feeds::out::render_board(&b.store, &anon, news.board, "https://x", now(4))
            .await
            .unwrap()
            .is_none()
    );
}
