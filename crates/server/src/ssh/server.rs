//! The listener: accepts TCP connections, enforces connection and authentication limits, and
//! hands each connection to a [`ClientHandler`].
//!
//! We run our own accept loop instead of russh's `run_on_address` so we can (a) cap concurrent
//! connections, (b) enforce an authentication deadline that russh does not provide, and (c) rate
//! limit each network before any SSH work is done (M1-C).

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use russh::server::{Config as RusshConfig, run_stream};
use russh::{Disconnect, MethodKind, MethodSet, SshId};
use russh_bbs_core::access::{Fingerprint, MemberDirectory};
use russh_bbs_core::waiting_room::{SystemClock, Verdict, WaitingRoomSource};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, oneshot, watch};
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::{Instant, timeout, timeout_at};
use tracing::{debug, info, warn};

use super::error::ServerError;
use super::handler::ClientHandler;
use super::host_key::HostKey;
use super::limits::Limits;
use super::registry::{SessionRegistry, SessionSnapshot};
use crate::waiting_room::Throttle;

/// How long a disconnected connection may take to wind down before we stop waiting for it.
const DISCONNECT_GRACE: Duration = Duration::from_secs(5);

/// Server banner. It names the software, not its version, to avoid advertising what to attack.
const SERVER_ID: &str = "SSH-2.0-russh-bbs";

/// Everything the SSH front end needs to start.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Address to listen on. Use port 0 in tests.
    pub listen: SocketAddr,
    /// Timeouts and caps.
    pub limits: Limits,
}

/// A running SSH server. Dropping it without [`SshServer::shutdown`] aborts the accept loop.
#[derive(Debug)]
pub struct SshServer {
    local_addr: SocketAddr,
    host_fingerprint: Fingerprint,
    registry: Arc<SessionRegistry>,
    stop: watch::Sender<bool>,
    task: JoinHandle<()>,
}

impl SshServer {
    /// Bind and start serving. Every key authenticates, then `directory` decides its access.
    pub async fn start<D>(
        config: &ServerConfig,
        host_key: &HostKey,
        directory: Arc<D>,
    ) -> Result<Self, ServerError>
    where
        D: MemberDirectory + WaitingRoomSource + 'static,
    {
        let listener =
            TcpListener::bind(config.listen)
                .await
                .map_err(|source| ServerError::Bind {
                    addr: config.listen,
                    source,
                })?;
        let local_addr = listener.local_addr().map_err(|source| ServerError::Bind {
            addr: config.listen,
            source,
        })?;
        let russh_config = Arc::new(russh_config(host_key, &config.limits));
        let registry = Arc::new(SessionRegistry::default());
        let (stop, stopped) = watch::channel(false);
        let accept = AcceptLoop {
            listener,
            russh_config,
            limits: config.limits,
            directory,
            registry: Arc::clone(&registry),
            permits: Arc::new(Semaphore::new(config.limits.max_connections)),
            throttle: Arc::new(Throttle::new(
                config.limits.ip_rate,
                config.limits.key_rate,
                config.limits.max_tracked_sources,
                &SystemClock::new(),
            )),
        };
        let task = tokio::spawn(accept.run(stopped));
        info!(%local_addr, host_key = %host_key.fingerprint(), "ssh server listening");
        Ok(Self {
            local_addr,
            host_fingerprint: host_key.fingerprint().clone(),
            registry,
            stop,
            task,
        })
    }

    /// The address actually bound (useful when the port was 0).
    #[must_use]
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// The host key fingerprint clients will see.
    #[must_use]
    pub fn host_fingerprint(&self) -> &Fingerprint {
        &self.host_fingerprint
    }

    /// A snapshot of the live sessions.
    #[must_use]
    pub fn sessions(&self) -> Vec<SessionSnapshot> {
        self.registry.snapshots()
    }

    /// Stop accepting, disconnect every session and wait for the loop to finish.
    pub async fn shutdown(self) {
        // An error only means the loop already ended.
        let _ = self.stop.send(true);
        if let Err(error) = self.task.await {
            warn!(%error, "ssh accept loop ended abnormally");
        }
    }
}

/// Build the russh configuration: public-key auth only, our limits, the host key.
fn russh_config(host_key: &HostKey, limits: &Limits) -> RusshConfig {
    RusshConfig {
        server_id: SshId::Standard(SERVER_ID.into()),
        methods: MethodSet::from(&[MethodKind::PublicKey][..]),
        auth_rejection_time: limits.auth_rejection_time,
        // OpenSSH probes with "none" first; answering that instantly keeps logins fast while
        // real failed attempts still pay `auth_rejection_time`.
        auth_rejection_time_initial: Some(Duration::ZERO),
        keys: vec![host_key.private_key().clone()],
        max_auth_attempts: usize::try_from(limits.max_auth_attempts).unwrap_or(usize::MAX),
        inactivity_timeout: Some(limits.idle_timeout),
        keepalive_interval: Some(limits.keepalive_interval),
        keepalive_max: 3,
        nodelay: true,
        ..RusshConfig::default()
    }
}

struct AcceptLoop<D> {
    listener: TcpListener,
    russh_config: Arc<RusshConfig>,
    limits: Limits,
    directory: Arc<D>,
    registry: Arc<SessionRegistry>,
    permits: Arc<Semaphore>,
    throttle: Arc<Throttle>,
}

impl<D: MemberDirectory + WaitingRoomSource + 'static> AcceptLoop<D> {
    async fn run(self, mut stopped: watch::Receiver<bool>) {
        // The JoinSet owns every connection task; dropping it on exit aborts them all.
        let mut connections = JoinSet::new();
        loop {
            tokio::select! {
                _ = stopped.changed() => break,
                Some(_) = connections.join_next() => {}
                accepted = self.listener.accept() => match accepted {
                    Ok((stream, peer)) => self.admit(&mut connections, stream, peer, &stopped),
                    Err(error) => {
                        warn!(%error, "accept failed");
                        // Avoid a hot loop on persistent errors such as fd exhaustion.
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                },
            }
        }
        connections.shutdown().await;
    }

    /// Decide whether to take a new connection and, if so, start serving it.
    fn admit(
        &self,
        connections: &mut JoinSet<()>,
        stream: TcpStream,
        peer: SocketAddr,
        stopped: &watch::Receiver<bool>,
    ) {
        // Per-network limit, before any handler or task exists. A dropped connection costs us
        // one accept and nothing else; the client sees the connection close. Logged at debug
        // so a flood does not become a log flood.
        if let Verdict::Limited { retry_after } = self.throttle.admit_connection(peer.ip()) {
            debug!(%peer, ?retry_after, "connection rate limit; dropping connection");
            return;
        }
        let Ok(permit) = Arc::clone(&self.permits).try_acquire_owned() else {
            warn!(%peer, "connection limit reached; dropping connection");
            return;
        };
        debug!(%peer, "connection accepted");
        let connection = Connection {
            config: Arc::clone(&self.russh_config),
            auth_timeout: self.limits.auth_timeout,
            directory: Arc::clone(&self.directory),
            registry: Arc::clone(&self.registry),
            throttle: Arc::clone(&self.throttle),
            max_form_submissions: self.limits.max_form_submissions,
            stopped: stopped.clone(),
            _permit: permit,
        };
        connections.spawn(connection.serve(stream, peer));
    }
}

/// One accepted connection and what it needs to run.
struct Connection<D> {
    config: Arc<RusshConfig>,
    auth_timeout: Duration,
    directory: Arc<D>,
    registry: Arc<SessionRegistry>,
    throttle: Arc<Throttle>,
    max_form_submissions: u32,
    stopped: watch::Receiver<bool>,
    /// Released when the connection ends, freeing a slot under `max_connections`.
    _permit: OwnedSemaphorePermit,
}

impl<D: MemberDirectory + WaitingRoomSource + 'static> Connection<D> {
    async fn serve(mut self, stream: TcpStream, peer: SocketAddr) {
        let deadline = Instant::now() + self.auth_timeout;
        let (authenticated_tx, authenticated_rx) = oneshot::channel();
        let guard = self.registry.register(Some(peer));
        let handler = ClientHandler::new(
            Arc::clone(&self.directory),
            Some(peer),
            guard,
            authenticated_tx,
            Arc::clone(&self.throttle),
            self.max_form_submissions,
        );

        let started = timeout_at(
            deadline,
            run_stream(Arc::clone(&self.config), stream, handler),
        );
        let mut session = match started.await {
            Ok(Ok(session)) => session,
            Ok(Err(error)) => {
                debug!(%peer, %error, "ssh handshake failed");
                return;
            }
            Err(_) => {
                info!(%peer, "handshake timed out");
                return;
            }
        };
        let handle = session.handle();
        let mut auth_wait = Box::pin(timeout_at(deadline, authenticated_rx));
        let mut authed = false;
        let mut disconnecting = false;

        loop {
            tokio::select! {
                result = &mut session => {
                    if let Err(error) = result {
                        debug!(%peer, %error, "session ended with error");
                    }
                    break;
                }
                outcome = &mut auth_wait, if !authed && !disconnecting => {
                    if matches!(outcome, Ok(Ok(()))) {
                        authed = true;
                    } else {
                        info!(%peer, "authentication timed out; disconnecting");
                        disconnect(&handle, "authentication timed out").await;
                        disconnecting = true;
                    }
                }
                _ = self.stopped.changed(), if !disconnecting => {
                    disconnect(&handle, "server shutting down").await;
                    disconnecting = true;
                }
                () = tokio::time::sleep(DISCONNECT_GRACE), if disconnecting => break,
            }
        }
        debug!(%peer, "connection closed");
    }
}

async fn disconnect(handle: &russh::server::Handle, reason: &str) {
    // The connection may already be gone; the grace timer in `serve` covers a stuck one.
    let _ = timeout(
        DISCONNECT_GRACE,
        handle.disconnect(Disconnect::ByApplication, reason.to_owned(), String::new()),
    )
    .await;
}
