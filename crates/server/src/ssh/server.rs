//! The listener: accepts TCP connections, enforces connection and authentication limits, and
//! hands each connection to a [`ClientHandler`].
//!
//! We run our own accept loop instead of russh's `run_on_address` so we can (a) cap concurrent
//! connections, (b) enforce an authentication deadline that russh does not provide, and (c) rate
//! limit each network before any SSH work is done (M1-C).

use std::net::{Shutdown, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use russh::server::{Config as RusshConfig, run_stream};
use russh::{Disconnect, MethodKind, MethodSet, SshId};
use russh_bbs_core::access::Fingerprint;
use russh_bbs_core::waiting_room::{SystemClock, Verdict};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, oneshot, watch};
use tokio::task::{JoinHandle, JoinSet};
use tokio::time::{Instant, timeout, timeout_at};
use tracing::{debug, info, warn};

use super::Backend;
use super::abort::Abort;
use super::error::ServerError;
use super::handler::{ClientHandler, HandlerParts};
use super::host_key::HostKey;
use super::limits::Limits;
use super::password::PasswordLogin;
use super::registry::{SessionRegistry, SessionSnapshot};
use crate::chat::Hub;
use crate::founder::Founder;
use crate::ui::Ui;
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
    pub async fn start<D: Backend>(
        config: &ServerConfig,
        host_key: &HostKey,
        directory: Arc<D>,
    ) -> Result<Self, ServerError> {
        Self::start_with_founder(config, host_key, directory, Arc::new(Founder::closed())).await
    }

    /// Like [`SshServer::start`], with a live founder claim (M1-D). `ssh claim@host` opens the
    /// token prompt; with a closed `founder` every token is rejected.
    pub async fn start_with_founder<D: Backend>(
        config: &ServerConfig,
        host_key: &HostKey,
        directory: Arc<D>,
        founder: Arc<Founder>,
    ) -> Result<Self, ServerError> {
        // A bad menu tree or string table stops the boot here, naming the key (M2-A).
        let ui = Arc::new(Ui::standard()?);
        Self::start_with_ui(config, host_key, directory, founder, ui).await
    }

    /// Like [`SshServer::start_with_founder`], with the screen framework's [`Ui`] chosen by the
    /// caller (M3-A): `main` passes one that also holds the sysop's themes.
    pub async fn start_with_ui<D: Backend>(
        config: &ServerConfig,
        host_key: &HostKey,
        directory: Arc<D>,
        founder: Arc<Founder>,
        ui: Arc<Ui>,
    ) -> Result<Self, ServerError> {
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
            ui,
            directory,
            founder,
            registry: Arc::clone(&registry),
            permits: Arc::new(Semaphore::new(config.limits.max_connections)),
            throttle: Arc::new(Throttle::new(
                config.limits.ip_rate,
                config.limits.key_rate,
                config.limits.max_tracked_sources,
                &SystemClock::new(),
            )),
            passwords: Arc::new(PasswordLogin::new(&config.limits)),
            chat: Arc::clone(registry.chat()),
        };
        let task = tokio::spawn(accept.run(stopped));
        let limits = &config.limits;
        if limits.allow_password_login {
            warn!(
                "SSH password login is ON (ssh.allow_password_login, D28): members who set a password can sign in with it"
            );
        }
        info!(
            %local_addr,
            host_key = %host_key.fingerprint(),
            max_connections = limits.max_connections,
            password_login = limits.allow_password_login,
            render_cap = %format_args!("{}x{}", limits.render_cap.cols(), limits.render_cap.rows()),
            render_memory_ceiling_mib = render_ceiling_mib(limits),
            "ssh server listening"
        );
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

    /// The registry of live sessions. The telnet transport registers its sessions here too, so
    /// the concurrent-session limit spans both (M3-H).
    #[must_use]
    pub fn registry(&self) -> Arc<SessionRegistry> {
        Arc::clone(&self.registry)
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

/// Build the russh configuration: public-key auth (and `password` when opted in), our limits,
/// the host key.
fn russh_config(host_key: &HostKey, limits: &Limits) -> RusshConfig {
    RusshConfig {
        server_id: SshId::Standard(SERVER_ID.into()),
        // Public key only, unless the sysop opted in to passwords (D28): then `password` is
        // offered too. Nothing else is ever offered (`none` is answered, never accepted).
        methods: if limits.allow_password_login {
            MethodSet::from(&[MethodKind::PublicKey, MethodKind::Password][..])
        } else {
            MethodSet::from(&[MethodKind::PublicKey][..])
        },
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
    ui: Arc<Ui>,
    directory: Arc<D>,
    founder: Arc<Founder>,
    registry: Arc<SessionRegistry>,
    permits: Arc<Semaphore>,
    throttle: Arc<Throttle>,
    passwords: Arc<PasswordLogin>,
    chat: Arc<Hub>,
}

impl<D: Backend> AcceptLoop<D> {
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
        //
        // This is also the founder claim's gate (FIX-M1-L1, D65): the user name `claim` is not
        // known until the SSH handshake, so a connection cannot be told apart here and an
        // address that has used its allowance is dropped whatever it wanted. Kept on purpose;
        // `docs/deploy.md` has the operator's workaround.
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
            founder: Arc::clone(&self.founder),
            registry: Arc::clone(&self.registry),
            throttle: Arc::clone(&self.throttle),
            passwords: Arc::clone(&self.passwords),
            chat: Arc::clone(&self.chat),
            limits: self.limits,
            ui: Arc::clone(&self.ui),
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
    founder: Arc<Founder>,
    registry: Arc<SessionRegistry>,
    throttle: Arc<Throttle>,
    passwords: Arc<PasswordLogin>,
    chat: Arc<Hub>,
    limits: Limits,
    ui: Arc<Ui>,
    stopped: watch::Receiver<bool>,
    /// Released when the connection ends, freeing a slot under `max_connections`.
    _permit: OwnedSemaphorePermit,
}

impl<D: Backend> Connection<D> {
    async fn serve(mut self, stream: TcpStream, peer: SocketAddr) {
        let deadline = Instant::now() + self.auth_timeout;
        let (authenticated_tx, authenticated_rx) = oneshot::channel();
        let guard = self.registry.register(Some(peer));
        let abort = Arc::new(Abort::default());
        // A second handle on the socket, kept only to be able to cut the connection when a
        // session's own tasks ask for it (a peer that stopped reading).
        let (stream, killer) = match split_killer(stream) {
            Ok(pair) => pair,
            Err(error) => {
                warn!(%peer, %error, "could not prepare the connection; dropping it");
                return;
            }
        };
        let handler = ClientHandler::new(
            HandlerParts {
                directory: Arc::clone(&self.directory),
                founder: Arc::clone(&self.founder),
                throttle: Arc::clone(&self.throttle),
                passwords: Arc::clone(&self.passwords),
                chat: Arc::clone(&self.chat),
                limits: self.limits,
                ui: Arc::clone(&self.ui),
                abort: Arc::clone(&abort),
            },
            Some(peer),
            guard,
            authenticated_tx,
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
        let mut aborted = false;

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
                () = abort.triggered(), if !aborted => {
                    aborted = true;
                    // The polite disconnect would queue behind the data the peer is not
                    // reading. Closing the socket ends the session task at once.
                    let _ = killer.shutdown(Shutdown::Both);
                    disconnecting = true;
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

/// Split a socket into the stream the SSH session uses and a second handle on the same socket
/// that can shut it down from outside the session.
fn split_killer(stream: TcpStream) -> std::io::Result<(TcpStream, std::net::TcpStream)> {
    let std_stream = stream.into_std()?;
    let killer = std_stream.try_clone()?;
    Ok((TcpStream::from_std(std_stream)?, killer))
}

async fn disconnect(handle: &russh::server::Handle, reason: &str) {
    // The connection may already be gone; the grace timer in `serve` covers a stuck one.
    let _ = timeout(
        DISCONNECT_GRACE,
        handle.disconnect(Disconnect::ByApplication, reason.to_owned(), String::new()),
    )
    .await;
}

/// The most render memory every session together can hold, in MiB: the sysop's bill for
/// `max_connections` at `render_cap` (D53). Logged at start so the trade-off is visible.
fn render_ceiling_mib(limits: &Limits) -> usize {
    limits
        .render_cap
        .bytes_per_session()
        .saturating_mul(limits.max_connections)
        / (1024 * 1024)
}
