//! The SSH front end: a `russh` server with a persistent host key, public-key authentication,
//! and a locked-down PTY session.
//!
//! In this slice no member store exists, so every authenticated key is
//! [`Access::Restricted`](russh_bbs_core::access::Access::Restricted) and sees only a
//! placeholder waiting-room screen. Rules (who is restricted) live in `russh-bbs-core`; this
//! module is transport and rendering only.

mod error;
mod handler;
pub mod host_key;
pub mod limits;
pub mod registry;
mod screen;
mod server;
pub mod terminal;

pub use error::{ServerError, SessionError};
pub use host_key::{HostKey, HostKeyError};
pub use limits::Limits;
pub use registry::{SessionId, SessionRegistry, SessionSnapshot};
pub use server::{ServerConfig, SshServer};
