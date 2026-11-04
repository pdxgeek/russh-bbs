//! The SSH front end: a `russh` server with a persistent host key, public-key authentication,
//! and a locked-down PTY session.
//!
//! A key that is not a member sees the waiting room (M1-C). A member's key sees the login screen, the glyph
//! test on first login, and the main menu (M2-A) with terminal settings and logoff; the
//! real member experience arrives in later slices. The SSH user name
//! `claim` opens the founder token prompt for every key (D41). Rules (who is restricted, which screens exist,
//! rate limits) live in `russh-bbs-core`; this module is transport and rendering only.

pub(crate) mod abort;
pub(crate) mod budget;
mod error;
mod handler;
pub mod host_key;
pub mod limits;
pub(crate) mod password;
pub mod registry;
pub(crate) mod room;
pub(crate) mod screen;
mod server;
pub(crate) mod shell;
pub mod terminal;

pub use error::{ServerError, SessionError};
pub use host_key::{HostKey, HostKeyError};
pub use limits::Limits;
pub use registry::{Revocation, SessionId, SessionRef, SessionRegistry, SessionSnapshot};
pub use server::{ServerConfig, SshServer};
pub use terminal::RenderCap;

use russh_bbs_core::access::{AccessStore, MemberDirectory};
use russh_bbs_core::audit::AuditStore;
use russh_bbs_core::chat::RoomStore;
use russh_bbs_core::community::CommunityStore;
use russh_bbs_core::doors::DoorStore;
use russh_bbs_core::drafts::DraftStore;
use russh_bbs_core::feeds::FeedStore;
use russh_bbs_core::forums::ForumStore;
use russh_bbs_core::founder::FounderStore;
use russh_bbs_core::invites::InviteStore;
use russh_bbs_core::keys::KeyStore;
use russh_bbs_core::limits::LimitStore;
use russh_bbs_core::mail::MailStore;
use russh_bbs_core::passwords::PasswordStore;
use russh_bbs_core::profiles::ProfileStore;
use russh_bbs_core::queue::QueueStore;
use russh_bbs_core::registration::RegistrationStore;
use russh_bbs_core::terminal_settings::MemberSessions;
use russh_bbs_core::waiting_room::WaitingRoomSource;

/// Everything the SSH front end asks of the board's store: who is a member, the waiting room's
/// settings, the founder claim, member profiles, and a member's session (what the account holds, terminal
/// settings and login time), its limits and usage counters (M2-H), and the sysop queue of invite
/// requests (M2-E). The SQLite store is one; so is the empty `NoMembers` directory used by
/// tests.
pub trait Backend:
    MemberDirectory
    + WaitingRoomSource
    + FounderStore
    + MemberSessions
    + InviteStore
    + KeyStore
    + ProfileStore
    + PasswordStore
    + QueueStore
    + CommunityStore
    + RoomStore
    + RegistrationStore
    + LimitStore
    + AccessStore
    + DraftStore
    + ForumStore
    + FeedStore
    + MailStore
    + AuditStore
    + DoorStore
    + 'static
{
}

impl<T> Backend for T where
    T: MemberDirectory
        + WaitingRoomSource
        + FounderStore
        + MemberSessions
        + InviteStore
        + KeyStore
        + ProfileStore
        + PasswordStore
        + QueueStore
        + CommunityStore
        + RoomStore
        + RegistrationStore
        + LimitStore
        + AccessStore
        + DraftStore
        + ForumStore
        + FeedStore
        + MailStore
        + AuditStore
        + DoorStore
        + 'static
{
}
