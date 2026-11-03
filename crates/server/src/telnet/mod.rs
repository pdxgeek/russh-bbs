//! The telnet front end (M3-H, D27): an optional, **unencrypted** transport for the same member
//! screens, off by default.
//!
//! Telnet is plaintext, so this transport is deliberately narrow:
//!
//! - **Off unless the sysop turns it on** (`telnet.enabled`); when off no port is bound. When on,
//!   the start logs a loud warning and the board is in *low-security mode*: staff screens carry
//!   the `LOW-SECURITY` marker (see `Limits::low_security`).
//! - **A separate listener** with its own accept loop. It shares the SSH server's
//!   [`SessionRegistry`] (so the concurrent-session limit spans both transports) and nothing
//!   else with it.
//! - **Login is a password, and only if the sysop opted in a second time**
//!   (`telnet.allow_login`). Otherwise the listener shows a short refusal and closes. There is
//!   no waiting room, no founder claim, no registration and no key over telnet, and a member's
//!   password is never set or changed here (D27): the settings screen is built without its
//!   password page.
//! - **Same screens, same session code.** After login the member gets the very same [`Shell`]
//!   that SSH runs, through [`Output::Frames`]; nothing about the screens is forked. The
//!   differences are all subtractions: the roles of rank 40 and above (cosysop, sysop) are
//!   dropped from every permission snapshot the session loads, so a sysop who logs in over
//!   telnet is an ordinary member.
//! - **Every telnet screen shows `UNENCRYPTED`.** The last row of the window belongs to the
//!   transport: the layouts are given one row less and the marker is painted there after every
//!   frame, with the reserved style, by [`crate::ui::chrome::footer`]. The login prompt shows it
//!   too.
//! - **The protocol is hostile input.** See [`protocol`]: bounded decoder, one refusal per
//!   option, only NAWS read, window sizes clamped like SSH's `pty-req`.
//!
//! Limits are the SSH ones: the per-network connection rate, `max_connections` (its own count,
//! so a telnet flood cannot starve SSH), the authentication deadline, the per-session request
//! and input budgets (D49), the output stall and the idle timeout, and the render cap (D53).

mod connection;
mod login;
mod protocol;
mod staff;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use russh_bbs_core::waiting_room::{SystemClock, Verdict};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, watch};
use tokio::task::{JoinHandle, JoinSet};
use tracing::{debug, info, warn};

use crate::ssh::password::PasswordLogin;
use crate::ssh::{Backend, Limits, ServerError, SessionRegistry};
use crate::ui::Ui;
use crate::waiting_room::Throttle;

pub use staff::{STAFF_RANK, without_staff};

/// Everything the telnet front end needs to start.
#[derive(Debug, Clone)]
pub struct TelnetConfig {
    /// Address to listen on. Use port 0 in tests.
    pub listen: SocketAddr,
    /// Whether members may sign in with a password over telnet (`telnet.allow_login`, the second
    /// opt-in). When off, a connection is told so and closed.
    pub allow_login: bool,
    /// The SSH limits and timeouts, which telnet follows (see the module docs).
    pub limits: Limits,
}

/// A running telnet server. Dropping it without [`TelnetServer::shutdown`] aborts the accept loop.
#[derive(Debug)]
pub struct TelnetServer {
    local_addr: SocketAddr,
    stop: watch::Sender<bool>,
    task: JoinHandle<()>,
}

impl TelnetServer {
    /// Bind and start serving. `registry` is the SSH server's, so the session limits are shared.
    pub async fn start<D: Backend>(
        config: &TelnetConfig,
        directory: Arc<D>,
        registry: Arc<SessionRegistry>,
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
        let limits = config.limits;
        // Telnet gets its own password limiters and hashing pool, separate from SSH's: the
        // switch is `telnet.allow_login`, not `ssh.allow_password_login`.
        let passwords = Arc::new(PasswordLogin::new(&Limits {
            allow_password_login: config.allow_login,
            ..limits
        }));
        let (stop, stopped) = watch::channel(false);
        let accept = AcceptLoop {
            listener,
            limits,
            allow_login: config.allow_login,
            ui,
            directory,
            registry,
            permits: Arc::new(Semaphore::new(limits.max_connections)),
            throttle: Arc::new(Throttle::new(
                limits.ip_rate,
                limits.key_rate,
                limits.max_tracked_sources,
                &SystemClock::new(),
            )),
            passwords,
        };
        let task = tokio::spawn(accept.run(stopped));
        warn!(
            %local_addr,
            login = config.allow_login,
            "TELNET IS ON (telnet.enabled, D27): the board is in LOW-SECURITY MODE. Telnet \
             sessions are NOT encrypted: anyone on the network path can read them, a password \
             typed at the prompt included. Staff roles are refused over telnet."
        );
        if config.allow_login {
            warn!(
                "telnet login is ON (telnet.allow_login): members who set a password can sign in with it over an unencrypted connection"
            );
        }
        info!(
            %local_addr,
            max_connections = limits.max_connections,
            login = config.allow_login,
            "telnet server listening"
        );
        Ok(Self {
            local_addr,
            stop,
            task,
        })
    }

    /// The address actually bound (useful when the port was 0).
    #[must_use]
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Stop accepting, drop every connection and wait for the loop to finish.
    pub async fn shutdown(self) {
        // An error only means the loop already ended.
        let _ = self.stop.send(true);
        if let Err(error) = self.task.await {
            warn!(%error, "telnet accept loop ended abnormally");
        }
    }
}

struct AcceptLoop<D> {
    listener: TcpListener,
    limits: Limits,
    allow_login: bool,
    ui: Arc<Ui>,
    directory: Arc<D>,
    registry: Arc<SessionRegistry>,
    permits: Arc<Semaphore>,
    throttle: Arc<Throttle>,
    passwords: Arc<PasswordLogin>,
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
                    Ok((stream, peer)) => self.admit(&mut connections, stream, peer),
                    Err(error) => {
                        warn!(%error, "telnet accept failed");
                        // Avoid a hot loop on persistent errors such as fd exhaustion.
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                },
            }
        }
        connections.shutdown().await;
    }

    /// Decide whether to take a new connection and, if so, start serving it. The same two
    /// gates as SSH's accept loop, before any task or buffer exists.
    fn admit(&self, connections: &mut JoinSet<()>, stream: TcpStream, peer: SocketAddr) {
        if let Verdict::Limited { retry_after } = self.throttle.admit_connection(peer.ip()) {
            debug!(%peer, ?retry_after, "telnet connection rate limit; dropping connection");
            return;
        }
        let Ok(permit) = Arc::clone(&self.permits).try_acquire_owned() else {
            warn!(%peer, "telnet connection limit reached; dropping connection");
            return;
        };
        debug!(%peer, "telnet connection accepted");
        let connection = connection::Connection {
            directory: Arc::clone(&self.directory),
            registry: Arc::clone(&self.registry),
            passwords: Arc::clone(&self.passwords),
            limits: self.limits,
            allow_login: self.allow_login,
            ui: Arc::clone(&self.ui),
            _permit: permit,
        };
        connections.spawn(connection.serve(stream, peer));
    }
}

/// Released when a connection ends, freeing a slot under `max_connections`.
type Slot = OwnedSemaphorePermit;

#[cfg(test)]
mod tests;
