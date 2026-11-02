//! Errors for the SSH front end.

use std::io;
use std::net::SocketAddr;

use super::host_key::HostKeyError;

/// A failure inside one connection's handler. It ends that connection only.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    /// The SSH protocol layer reported an error.
    #[error("ssh protocol error: {0}")]
    Ssh(#[from] russh::Error),
    /// Drawing to the client's terminal failed.
    #[error("terminal render error: {0}")]
    Render(#[from] io::Error),
    /// The client kept sending past its request and input budgets (D46).
    #[error("session over its request budget")]
    OverBudget,
}

/// A failure starting the server.
#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    /// The host key could not be loaded or created.
    #[error(transparent)]
    HostKey(#[from] HostKeyError),
    /// The listen address could not be bound.
    #[error("cannot listen on {addr}: {source}")]
    Bind {
        /// The address we tried.
        addr: SocketAddr,
        /// The underlying error.
        #[source]
        source: io::Error,
    },
    /// A blocking startup task failed.
    #[error("startup task failed: {0}")]
    Join(String),
}
