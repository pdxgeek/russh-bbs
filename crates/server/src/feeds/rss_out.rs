//! RSS out: a small HTTP listener that serves public boards as RSS 2.0 (M3-G, D89).
//!
//! Off by default (`feeds.rss_out_enabled`); when off no port is bound. It speaks plain HTTP and
//! answers `GET /rss/<board id>` only. What it serves is decided in core
//! ([`russh_bbs_core::feeds::out`]) on an anonymous snapshot: a board that is absent, not public
//! or restricted gets the same `404` with the same body, and an id that is not public never
//! reaches the database (the check is on the cached snapshot). Documents are cached for a minute,
//! so a crowd of readers costs one render a minute per board. Connections are capped, each has a
//! header-read timeout and a total lifetime, and nothing is kept per connection.

use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use http_body_util::Full;
use hyper::body::{Bytes, Incoming};
use hyper::header::{ALLOW, CACHE_CONTROL, CONTENT_TYPE};
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::{TokioIo, TokioTimer};
use russh_bbs_core::access::{AccessStore, Permission, ResourceKind, Snapshot};
use russh_bbs_core::feeds::{FeedStore, out};
use russh_bbs_core::forums::{BoardId, ForumStore};
use russh_bbs_core::limits::{Clock, SystemClock};
use tokio::net::TcpListener;
use tokio::sync::{Semaphore, watch};
use tokio::task::JoinHandle;
use tracing::{info, warn};

/// How long a rendered document and the public snapshot are reused.
const CACHE_FOR: Duration = Duration::from_secs(60);
/// A connection that has not sent its request head by now is dropped.
const HEADER_TIMEOUT: Duration = Duration::from_secs(10);
/// A connection never lives longer than this.
const CONNECTION_LIFETIME: Duration = Duration::from_secs(30);
/// Most connections served at once.
const MAX_CONNECTIONS: usize = 64;

/// The listener's settings.
#[derive(Debug, Clone)]
pub struct RssOutConfig {
    /// Where to listen.
    pub listen: SocketAddr,
    /// The public address readers reach the listener at (no trailing slash).
    pub base_url: String,
}

/// A running listener.
#[derive(Debug)]
pub struct RssOut {
    local: SocketAddr,
    stop: watch::Sender<bool>,
    task: JoinHandle<()>,
}

struct State<S> {
    store: Arc<S>,
    base_url: String,
    cache: Mutex<Cache>,
}

#[derive(Default)]
struct Cache {
    snapshot: Option<(Instant, Snapshot)>,
    documents: HashMap<BoardId, (Instant, Bytes)>,
}

impl RssOut {
    /// Bind and serve.
    ///
    /// # Errors
    /// The bind error.
    pub async fn start<S>(config: &RssOutConfig, store: Arc<S>) -> std::io::Result<Self>
    where
        S: ForumStore + AccessStore + FeedStore + 'static,
    {
        let listener = TcpListener::bind(config.listen).await?;
        let local = listener.local_addr()?;
        let (stop, stopped) = watch::channel(false);
        let state = Arc::new(State {
            store,
            base_url: config.base_url.clone(),
            cache: Mutex::new(Cache::default()),
        });
        info!(%local, "RSS out is listening");
        let task = tokio::spawn(accept_loop(listener, state, stopped));
        Ok(Self { local, stop, task })
    }

    /// The bound address (the port when `0` was asked for).
    #[must_use]
    pub fn local_addr(&self) -> SocketAddr {
        self.local
    }

    /// Stop accepting and wait for the accept loop to end.
    pub async fn shutdown(self) {
        let _ = self.stop.send(true);
        let _ = self.task.await;
    }
}

async fn accept_loop<S>(
    listener: TcpListener,
    state: Arc<State<S>>,
    mut stopped: watch::Receiver<bool>,
) where
    S: ForumStore + AccessStore + FeedStore + 'static,
{
    let permits = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    loop {
        let (stream, _peer) = tokio::select! {
            _ = stopped.changed() => return,
            accepted = listener.accept() => match accepted {
                Ok(pair) => pair,
                Err(error) => {
                    warn!(%error, "RSS out could not accept a connection");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
            },
        };
        // Over the cap, the connection is closed at once rather than queued.
        let Ok(permit) = Arc::clone(&permits).try_acquire_owned() else {
            continue;
        };
        let state = Arc::clone(&state);
        tokio::spawn(async move {
            let _permit = permit;
            let service = service_fn(move |request| {
                let state = Arc::clone(&state);
                async move { Ok::<_, Infallible>(handle(&state, &request).await) }
            });
            let connection = hyper::server::conn::http1::Builder::new()
                .timer(TokioTimer::new())
                .header_read_timeout(HEADER_TIMEOUT)
                .keep_alive(false)
                .serve_connection(TokioIo::new(stream), service);
            let _ = tokio::time::timeout(CONNECTION_LIFETIME, connection).await;
        });
    }
}

fn text(status: StatusCode, body: &'static str) -> Response<Full<Bytes>> {
    let mut response = Response::new(Full::new(Bytes::from_static(body.as_bytes())));
    *response.status_mut() = status;
    let headers = response.headers_mut();
    headers.insert(
        CONTENT_TYPE,
        "text/plain; charset=utf-8"
            .parse()
            .unwrap_or_else(|_| hyper::header::HeaderValue::from_static("text/plain")),
    );
    headers.insert(
        "x-content-type-options",
        hyper::header::HeaderValue::from_static("nosniff"),
    );
    response
}

/// The one answer for everything that is not a public board's feed.
fn not_found() -> Response<Full<Bytes>> {
    text(StatusCode::NOT_FOUND, "Not found\n")
}

/// `/rss/<digits>` to a board id.
fn board_of(path: &str) -> Option<BoardId> {
    let digits = path.strip_prefix("/rss/")?;
    if digits.is_empty() || digits.len() > 18 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse::<i64>().ok().map(BoardId::from_raw)
}

async fn handle<S>(state: &State<S>, request: &Request<Incoming>) -> Response<Full<Bytes>>
where
    S: ForumStore + AccessStore + FeedStore + 'static,
{
    if request.method() != Method::GET {
        let mut response = text(StatusCode::METHOD_NOT_ALLOWED, "Method not allowed\n");
        response
            .headers_mut()
            .insert(ALLOW, hyper::header::HeaderValue::from_static("GET"));
        return response;
    }
    let Some(board) = board_of(request.uri().path()) else {
        return not_found();
    };
    match document(state, board).await {
        Ok(Some(body)) => {
            let mut response = Response::new(Full::new(body));
            let headers = response.headers_mut();
            headers.insert(
                CONTENT_TYPE,
                hyper::header::HeaderValue::from_static("application/rss+xml; charset=utf-8"),
            );
            headers.insert(
                CACHE_CONTROL,
                hyper::header::HeaderValue::from_static("public, max-age=60"),
            );
            headers.insert(
                "x-content-type-options",
                hyper::header::HeaderValue::from_static("nosniff"),
            );
            response
        }
        Ok(None) => not_found(),
        Err(error) => {
            warn!(%error, "RSS out could not read the store");
            text(StatusCode::INTERNAL_SERVER_ERROR, "Try again later\n")
        }
    }
}

async fn document<S>(
    state: &State<S>,
    board: BoardId,
) -> Result<Option<Bytes>, russh_bbs_core::store::StoreError>
where
    S: ForumStore + AccessStore + FeedStore + 'static,
{
    let now = Instant::now();
    let cached = {
        let cache = state.cache.lock().unwrap_or_else(PoisonError::into_inner);
        cache
            .documents
            .get(&board)
            .filter(|(at, _)| now.duration_since(*at) < CACHE_FOR)
            .map(|(_, body)| body.clone())
    };
    if let Some(body) = cached {
        return Ok(Some(body));
    }
    let snapshot = {
        let fresh = {
            let cache = state.cache.lock().unwrap_or_else(PoisonError::into_inner);
            cache
                .snapshot
                .as_ref()
                .filter(|(at, _)| now.duration_since(*at) < CACHE_FOR)
                .map(|(_, s)| s.clone())
        };
        match fresh {
            Some(snapshot) => snapshot,
            None => {
                let snapshot = out::public_snapshot(&*state.store).await?;
                let mut cache = state.cache.lock().unwrap_or_else(PoisonError::into_inner);
                cache.snapshot = Some((now, snapshot.clone()));
                // Boards that stopped being public are dropped with the old snapshot.
                cache.documents.clear();
                snapshot
            }
        }
    };
    // An id that is not public never reaches the database.
    let reach = snapshot.reach(Permission::ForumRead, ResourceKind::Forum);
    let Some(resource) = board.resource() else {
        return Ok(None);
    };
    if !reach.contains(resource.id) {
        return Ok(None);
    }
    let Some(xml) = out::render_board(
        &*state.store,
        &snapshot,
        board,
        &state.base_url,
        SystemClock.now(),
    )
    .await?
    else {
        return Ok(None);
    };
    let body = Bytes::from(xml);
    let mut cache = state.cache.lock().unwrap_or_else(PoisonError::into_inner);
    if cache.documents.len() < 1_000 {
        cache.documents.insert(board, (now, body.clone()));
    }
    Ok(Some(body))
}
