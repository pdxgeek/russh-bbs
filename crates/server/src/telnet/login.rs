//! The login prompt: a user name and a password, typed at a plain-text prompt.
//!
//! The check is core's [`authenticate`], exactly as SSH password login uses it: an unknown name,
//! a name with no password, a suspended account and a wrong password all get one refusal and one
//! argon2id verification each. In front of it are telnet's own limits (a burst per handle and
//! per network, [`PasswordLogin::admit`]), a pause after each failure, and a cap on attempts per
//! connection. Nothing typed is logged.
//!
//! Input goes through the same [`KeyParser`](crate::ssh::terminal::KeyParser) as the screens, so
//! an escape sequence (an arrow key) is swallowed, not typed into the field. Only printable ASCII
//! is accepted, which means a password with other characters cannot be used over telnet.

use std::net::SocketAddr;

use russh_bbs_core::passwords::{Login, authenticate};
use russh_bbs_core::store::Account;
use russh_bbs_core::waiting_room::Verdict;
use tokio::time::{Instant, sleep, timeout, timeout_at};
use tracing::{info, warn};
use zeroize::Zeroizing;

use super::connection::{Connection, Link, PASSWORD_CHECK_TIMEOUT};
use crate::ssh::Backend;
use crate::ssh::terminal::Key;

/// Most characters of a user name read at the prompt (core refuses longer names anyway).
const MAX_HANDLE: usize = 64;
/// Most characters of a password read at the prompt (the longest core accepts to set).
const MAX_PASSWORD: usize = russh_bbs_core::passwords::MAX_CHARS;

/// What the prompt shows first. Fixed chrome, not content.
const BANNER: [&str; 8] = [
    "russh-bbs, over telnet",
    "",
    "THIS CONNECTION IS NOT ENCRYPTED. Anyone on the network between you",
    "and this board can read what you type, your password included.",
    "Use ssh if you can.",
    "",
    "New here? Type new as your login name to join.",
    "",
];

/// The login name that opens the waiting room instead of asking for a password (D94). Reserved
/// as a handle, so no member's login is taken by it.
const JOIN_NAME: &str = "new";

/// Where the login prompt led.
pub(super) enum Entry {
    /// A password matched an active member.
    Member(Account),
    /// The visitor asked to join: the waiting room.
    Join,
}

/// The one answer to every failed login.
const REFUSED: &str = "Login incorrect.\r\n\r\n";

/// A line typed at a prompt.
enum Line {
    Text(Zeroizing<String>),
    /// The person asked to leave (Ctrl-C, Ctrl-D) or the client went away.
    Quit,
}

impl<D: Backend> Connection<D> {
    /// Ask for a name and a password until one matches an active member, the attempts run out
    /// or the client leaves. The caller bounds the whole thing with the authentication deadline.
    pub(super) async fn login(&self, link: &mut Link, peer: SocketAddr) -> Option<Entry> {
        link.screen_text(&BANNER).await;
        let mut attempts = 0_u32;
        loop {
            link.send_text("login: ").await;
            let Line::Text(user) = link.read_line(true, MAX_HANDLE).await else {
                return None;
            };
            if user.trim().eq_ignore_ascii_case(JOIN_NAME) {
                link.send_text("\r\n").await;
                return Some(Entry::Join);
            }
            link.send_text("\r\npassword: ").await;
            let Line::Text(password) = link.read_line(false, MAX_PASSWORD).await else {
                return None;
            };
            link.send_text("\r\n").await;
            attempts += 1;
            if let Some(account) = self.check(link, peer, &user, &password).await {
                info!(session = ?link.session.id(), %peer, account = %account.id, "telnet password authenticated");
                return Some(Entry::Member(account));
            }
            // The pause makes guessing slow; the same time for every failure.
            sleep(self.limits.auth_rejection_time).await;
            link.send_text(REFUSED).await;
            if attempts >= self.limits.max_auth_attempts {
                info!(session = ?link.session.id(), %peer, "too many telnet login attempts; disconnecting");
                link.send_text("Too many failed attempts.\r\n").await;
                return None;
            }
        }
    }

    /// One attempt. Limited, wrong, slow and broken all return `None`: the person sees the same
    /// refusal for every one of them.
    async fn check(
        &self,
        link: &Link,
        peer: SocketAddr,
        user: &str,
        password: &str,
    ) -> Option<Account> {
        // Limits first, for every name alike (known or not), so a limited reply says nothing
        // about who is a member.
        if let Verdict::Limited { retry_after } = self.passwords.admit(Some(peer.ip()), user) {
            info!(session = ?link.session.id(), %peer, ?retry_after, "telnet password rate limit; rejecting");
            return None;
        }
        let checked = timeout(
            PASSWORD_CHECK_TIMEOUT,
            authenticate(&*self.directory, self.passwords.pool(), user, password),
        )
        .await;
        match checked {
            Ok(Ok(Login::Accepted(account))) => Some(account),
            Ok(Ok(Login::Denied)) => {
                info!(session = ?link.session.id(), %peer, "telnet password rejected");
                None
            }
            Ok(Err(error)) => {
                warn!(session = ?link.session.id(), %error, "telnet password check failed in the store; rejecting");
                None
            }
            Err(_) => {
                warn!(session = ?link.session.id(), "telnet password check timed out; rejecting");
                None
            }
        }
    }
}

impl Link {
    /// Read one line of at most `max` characters. With `echo` the typed characters are shown
    /// (the client does not echo: we offered to); without it nothing is shown.
    async fn read_line(&mut self, echo: bool, max: usize) -> Line {
        let mut line = Zeroizing::new(String::new());
        loop {
            while let Some(key) = self.pending.pop_front() {
                match key {
                    Key::Char(c) => {
                        if line.chars().count() < max {
                            line.push(c);
                            if echo {
                                self.send_text(c.encode_utf8(&mut [0; 4])).await;
                            }
                        }
                    }
                    Key::Backspace => {
                        if line.pop().is_some() && echo {
                            self.send_text("\x08 \x08").await;
                        }
                    }
                    Key::Ctrl('u') => {
                        let erase = line.chars().count();
                        line.clear();
                        if echo && erase > 0 {
                            self.send_text(&"\x08 \x08".repeat(erase)).await;
                        }
                    }
                    Key::Enter => return Line::Text(line),
                    Key::Ctrl('c' | 'd') => return Line::Quit,
                    Key::Ctrl(_)
                    | Key::Escape
                    | Key::Up
                    | Key::Down
                    | Key::Left
                    | Key::Right
                    | Key::Home
                    | Key::End
                    | Key::Delete
                    | Key::PageUp
                    | Key::PageDown
                    | Key::PasteBreak
                    | Key::Other => {}
                }
            }
            let decoded = if let Some(deadline) = self.keys.deadline() {
                let Ok(decoded) = timeout_at(Instant::from_std(deadline), self.next()).await else {
                    // A held Esc was the Escape key all along.
                    let keys = self.keys.expire();
                    self.pending.extend(keys);
                    continue;
                };
                decoded
            } else {
                self.next().await
            };
            let Some(decoded) = decoded else {
                return Line::Quit;
            };
            self.feed_back(&decoded);
        }
    }
}
