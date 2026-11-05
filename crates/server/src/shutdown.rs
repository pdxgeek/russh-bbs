//! Process shutdown: wait for SIGTERM or SIGINT, then stop in a bounded time (D25).

use std::time::Duration;

use russh_bbs_store::Close;
use tracing::{info, warn};

use crate::ssh::SshServer;

/// How long shutdown may take before the process exits anyway. Under Docker's default 10 s
/// stop timeout, so a slow close ends in a clean exit rather than SIGKILL.
pub const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(8);

/// The process's stop signals, installed **before** anything listens. Until a handler exists a
/// SIGTERM takes its default action and kills the process with a signal status, so installing
/// late left a window after "ssh server listening" in which `docker stop` did not exit 0.
#[derive(Debug)]
pub struct Signals {
    #[cfg(unix)]
    term: tokio::signal::unix::Signal,
    #[cfg(unix)]
    interrupt: tokio::signal::unix::Signal,
}

impl Signals {
    /// Register the handlers for SIGTERM and SIGINT.
    pub fn install() -> std::io::Result<Self> {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{SignalKind, signal};
            Ok(Self {
                term: signal(SignalKind::terminate())?,
                interrupt: signal(SignalKind::interrupt())?,
            })
        }
        #[cfg(not(unix))]
        {
            Ok(Self {})
        }
    }

    /// Resolve when the process is asked to stop: SIGTERM (`docker stop`, systemd) or SIGINT
    /// (Ctrl-C). A signal that arrived after [`Signals::install`] is not lost.
    pub async fn wait(&mut self) -> std::io::Result<()> {
        #[cfg(unix)]
        {
            tokio::select! {
                _ = self.term.recv() => info!("received SIGTERM"),
                _ = self.interrupt.recv() => info!("received SIGINT"),
            }
            Ok(())
        }
        #[cfg(not(unix))]
        {
            tokio::signal::ctrl_c().await
        }
    }
}

/// Stop accepting, close the sessions, then close the store (SQLite checkpoints its WAL; Postgres closes the pool), all within
/// `deadline`. Returns `false` when the deadline passed first.
pub async fn graceful<S: Close>(server: SshServer, store: &S, deadline: Duration) -> bool {
    let work = async {
        server.shutdown().await;
        store.close().await;
    };
    if tokio::time::timeout(deadline, work).await.is_ok() {
        info!("shutdown complete");
        true
    } else {
        warn!(?deadline, "shutdown timed out; exiting anyway");
        false
    }
}
