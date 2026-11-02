//! The SSH front end: a `russh` server with a persistent host key, public-key authentication,
//! and a locked-down PTY session.
//!
//! Every authenticated key gets a restricted session that shows the waiting room (M1-C); the
//! member experience arrives in later slices. Rules (who is restricted, which screens exist,
//! rate limits) live in `russh-bbs-core`; this module is transport and rendering only.

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

use russh_bbs_core::access::{MemberDirectory, RoleDirectory};
use russh_bbs_core::founder::FounderStore;
use russh_bbs_core::waiting_room::WaitingRoomSource;

/// Everything the SSH front end asks of the board's store: who is a member and in what role,
/// the waiting room's settings, and the founder claim. The SQLite store is one; so is the
/// empty `NoMembers` directory used by tests.
pub trait Backend:
    MemberDirectory + RoleDirectory + WaitingRoomSource + FounderStore + 'static
{
}

impl<T> Backend for T where
    T: MemberDirectory + RoleDirectory + WaitingRoomSource + FounderStore + 'static
{
}
