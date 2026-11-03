//! Member passwords and opt-in SSH password login over a real russh client against the real
//! SQLite store (M3-D, D28, D79): off by default and not offered when off; the same plain refusal
//! for an unknown name, a wrong password, an account with no password and an account that is not
//! active; per-handle and per-network limits; setting and clearing from the settings screen with
//! the current password required in a password session; nothing readable stored or logged.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::too_many_lines,
    clippy::unused_async_trait_impl,
    clippy::assert_is_empty,
    missing_docs
)]

mod common;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::{Client, fast_limits, new_client_key};
use russh::client::{self, AuthResult, Handle};
use russh::keys::{HashAlg, PrivateKey, PrivateKeyWithHashAlg};
use russh::{Channel, ChannelMsg, MethodKind};
use russh_bbs_core::access::{Fingerprint, Permission, Slug};
use russh_bbs_core::icons::Tier;
use russh_bbs_core::passwords::PasswordStore;
use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::store::{AccountId, AccountState, AccountStore, NewAccount};
use russh_bbs_core::terminal_settings::{TerminalSettings, TerminalSettingsStore};
use russh_bbs_core::waiting_room::RatePolicy;
use russh_bbs_server::ssh::{HostKey, Limits, ServerConfig, SshServer};
use russh_bbs_store::SqliteStore;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};
use tempfile::TempDir;
use tokio::time::timeout;

const SECRET: &str = "correct horse battery";
const NEXT: &str = "a brand new passphrase";

struct Rig {
    server: SshServer,
    store: SqliteStore,
    db: SqlitePool,
    dir: TempDir,
}

fn lenient(password_login: bool) -> Limits {
    let loose = RatePolicy::new(1_000, Duration::from_millis(1));
    Limits {
        ip_rate: loose,
        key_rate: loose,
        allow_password_login: password_login,
        password_account_rate: loose,
        password_network_rate: loose,
        ..fast_limits()
    }
}

impl Rig {
    async fn start(limits: Limits) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bbs.db");
        let store = SqliteStore::open(&path).await.unwrap();
        let db = SqlitePool::connect_with(SqliteConnectOptions::new().filename(&path))
            .await
            .unwrap();
        let host_key = HostKey::load_or_create(&dir.path().join("host_key")).unwrap();
        let config = ServerConfig {
            listen: "127.0.0.1:0".parse().unwrap(),
            limits,
        };
        let server = SshServer::start(&config, &host_key, Arc::new(store.clone()))
            .await
            .unwrap();
        Self {
            server,
            store,
            db,
            dir,
        }
    }

    fn addr(&self) -> SocketAddr {
        self.server.local_addr()
    }

    /// An account with a key; terminal settings already chosen so login goes to the menu.
    async fn account(&self, name: &str, state: AccountState) -> (AccountId, PrivateKey) {
        let key = new_client_key();
        let account = self
            .store
            .create_account(NewAccount {
                username: sanitize_username(name).unwrap(),
                role: Slug::MEMBER,
                state,
            })
            .await
            .unwrap();
        let fp =
            Fingerprint::parse(&key.public_key().fingerprint(HashAlg::Sha256).to_string()).unwrap();
        self.store
            .attach_fingerprint(account.id, &fp)
            .await
            .unwrap();
        let settings = TerminalSettings::new(None, None, true, true, Some(Tier::Ascii)).unwrap();
        self.store
            .set_terminal_settings(account.id, &settings)
            .await
            .unwrap();
        (account.id, key)
    }

    async fn member(&self, name: &str) -> (AccountId, PrivateKey) {
        self.account(name, AccountState::Active).await
    }

    /// Give an account a password the way the screen would (hash and store).
    async fn give_password(&self, id: AccountId, password: &str) {
        let hash = russh_bbs_core::passwords::hash::hash(password).unwrap();
        self.store
            .set_hash(
                id,
                Some(&hash),
                &russh_bbs_core::audit::events::password_set(id, "key session"),
            )
            .await
            .unwrap();
    }

    async fn hash_of(&self, id: AccountId) -> Option<String> {
        self.store.stored_hash(id).await.unwrap()
    }

    async fn audit_actions(&self) -> Vec<String> {
        sqlx::query_scalar::<_, String>(
            "SELECT action FROM audit_log WHERE action LIKE 'password.%' ORDER BY id",
        )
        .fetch_all(&self.db)
        .await
        .unwrap()
    }
}

async fn by_key(addr: SocketAddr, key: &PrivateKey) -> Handle<Client> {
    let mut session = common::connect(addr).await;
    let result = session
        .authenticate_publickey(
            "anyone",
            PrivateKeyWithHashAlg::new(Arc::new(key.clone()), None),
        )
        .await
        .unwrap();
    assert!(matches!(result, AuthResult::Success), "{result:?}");
    session
}

/// One password attempt on a fresh connection.
async fn try_password(
    addr: SocketAddr,
    user: &str,
    password: &str,
) -> (Handle<Client>, AuthResult) {
    let mut session = common::connect(addr).await;
    let result = session.authenticate_password(user, password).await.unwrap();
    (session, result)
}

async fn succeeds(addr: SocketAddr, user: &str, password: &str) -> Handle<Client> {
    let (session, result) = try_password(addr, user, password).await;
    assert!(matches!(result, AuthResult::Success), "{user}: {result:?}");
    session
}

fn failed(result: &AuthResult) -> bool {
    matches!(result, AuthResult::Failure { .. })
}

async fn raw(channel: &mut Channel<client::Msg>) -> String {
    let mut raw = String::new();
    while let Ok(msg) = timeout(Duration::from_millis(250), channel.wait()).await {
        match msg {
            Some(ChannelMsg::Data { data }) => raw.push_str(&String::from_utf8_lossy(&data)),
            Some(ChannelMsg::Close | ChannelMsg::Eof) | None => break,
            Some(_) => {}
        }
    }
    raw
}

fn words_of(raw: &str) -> String {
    let mut out = String::new();
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for n in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&n) {
                        break;
                    }
                }
            }
            out.push(' ');
        } else if c.is_control() || "│┌┐└┘─|+-".contains(c) {
            out.push(' ');
        } else {
            out.push(c);
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The whole current screen: a window change makes the server clear and redraw everything.
async fn full(channel: &mut Channel<client::Msg>) -> String {
    channel.window_change(80, 24, 0, 0).await.unwrap();
    words_of(&raw(channel).await)
}

/// Send keys and return the whole screen afterwards (a redraw only repaints what changed, so
/// the bytes of one frame alone split words).
async fn send(channel: &mut Channel<client::Msg>, bytes: &str) -> String {
    channel.data_bytes(bytes.as_bytes().to_vec()).await.unwrap();
    raw(channel).await;
    full(channel).await
}

/// Send keys that make the server hash (slow in a debug build) and wait until the screen shows
/// `needle`, returning the screen. Panics with the last screen if it never does.
async fn send_until(channel: &mut Channel<client::Msg>, bytes: &str, needle: &str) -> String {
    channel.data_bytes(bytes.as_bytes().to_vec()).await.unwrap();
    let mut screen = String::new();
    for _ in 0..40 {
        raw(channel).await;
        screen = full(channel).await;
        if screen.contains(needle) {
            return screen;
        }
    }
    panic!("never saw {needle:?}; last screen: {screen}");
}

/// Past the login screen, at the main menu.
async fn at_menu(session: &Handle<Client>) -> Channel<client::Msg> {
    let mut channel = common::open_shell(session, 80, 24).await;
    words_of(&raw(&mut channel).await);
    send(&mut channel, "x").await;
    channel
}

/// From the main menu to the password page.
async fn to_password_page(channel: &mut Channel<client::Msg>) -> String {
    send(channel, "s").await;
    send(channel, "s").await;
    full(channel).await
}

// ---------------------------------------------------------------- the method and who gets in

#[tokio::test]
async fn password_login_is_off_by_default_and_the_method_is_not_offered() {
    let rig = Rig::start(fast_limits()).await;
    assert!(!Limits::default().allow_password_login);
    let (id, _) = rig.member("alice").await;
    rig.give_password(id, SECRET).await;
    let mut session = common::connect(rig.addr()).await;
    let AuthResult::Failure {
        remaining_methods, ..
    } = session.authenticate_none("alice").await.unwrap()
    else {
        panic!("none must not succeed");
    };
    assert_eq!(
        remaining_methods.iter().copied().collect::<Vec<_>>(),
        vec![MethodKind::PublicKey]
    );
    // The right password does nothing when the option is off.
    let result = session
        .authenticate_password("alice", SECRET)
        .await
        .unwrap();
    assert!(failed(&result), "{result:?}");
}

#[tokio::test]
async fn when_on_the_method_is_offered_and_the_right_password_signs_in() {
    let rig = Rig::start(lenient(true)).await;
    let mut session = common::connect(rig.addr()).await;
    let AuthResult::Failure {
        remaining_methods, ..
    } = session.authenticate_none("alice").await.unwrap()
    else {
        panic!("none must not succeed");
    };
    let offered: Vec<_> = remaining_methods.iter().copied().collect();
    assert!(offered.contains(&MethodKind::PublicKey) && offered.contains(&MethodKind::Password));
    assert!(!offered.contains(&MethodKind::KeyboardInteractive));

    let (id, _) = rig.member("alice").await;
    rig.give_password(id, SECRET).await;
    // Case-insensitive handle, as handles are unique ignoring case.
    let session = succeeds(rig.addr(), "Alice", SECRET).await;
    let mut channel = at_menu(&session).await;
    let screen = full(&mut channel).await;
    assert!(screen.contains("Main menu"), "{screen}");
    assert!(
        screen.contains("The server can read what you do here."),
        "{screen}"
    );
    // The registry shows a signed-in member with no key.
    let sessions = rig.server.sessions();
    let mine = sessions.iter().find(|s| s.shell_open).unwrap();
    assert_eq!(mine.fingerprint, None);
    assert_eq!(mine.access, Some(russh_bbs_core::access::Access::Member));
}

#[tokio::test]
async fn every_refusal_looks_the_same_to_the_client() {
    let rig = Rig::start(lenient(true)).await;
    let (alice, _) = rig.member("alice").await;
    rig.give_password(alice, SECRET).await;
    rig.member("nopass").await;
    let (locked, _) = rig.account("locked", AccountState::Suspended).await;
    rig.give_password(locked, SECRET).await;
    let (waiting, _) = rig.account("newbie", AccountState::Pending).await;
    rig.give_password(waiting, SECRET).await;

    let attempts = [
        ("alice", "not the password"),
        ("alice", ""),
        ("nobody", SECRET),
        ("nopass", SECRET),
        ("nopass", ""),
        ("locked", SECRET),
        ("newbie", SECRET),
        ("", SECRET),
        ("a\u{1b}[31mb", SECRET),
    ];
    let mut shapes = Vec::new();
    for (user, password) in attempts {
        let (_session, result) = try_password(rig.addr(), user, password).await;
        let AuthResult::Failure {
            remaining_methods,
            partial_success,
        } = result
        else {
            panic!("{user:?} must be refused");
        };
        shapes.push((
            remaining_methods.iter().copied().collect::<Vec<_>>(),
            partial_success,
        ));
    }
    assert!(
        shapes.windows(2).all(|pair| pair[0] == pair[1]),
        "{shapes:?}"
    );
    // And nobody was signed in by any of it.
    assert!(rig.server.sessions().iter().all(|s| s.access.is_none()));
}

#[tokio::test]
async fn an_unknown_name_costs_about_what_a_wrong_password_costs() {
    let rig = Rig::start(lenient(true)).await;
    let (alice, _) = rig.member("alice").await;
    rig.give_password(alice, SECRET).await;
    let median = |mut times: Vec<Duration>| {
        times.sort();
        times[times.len() / 2]
    };
    let mut wrong = Vec::new();
    let mut unknown = Vec::new();
    let mut absent = Vec::new();
    rig.member("bare").await;
    for _ in 0..5 {
        let start = Instant::now();
        let (_s, result) = try_password(rig.addr(), "alice", "wrong password here").await;
        assert!(failed(&result));
        wrong.push(start.elapsed());
        let start = Instant::now();
        let (_s, result) = try_password(rig.addr(), "nobody", "wrong password here").await;
        assert!(failed(&result));
        unknown.push(start.elapsed());
        let start = Instant::now();
        let (_s, result) = try_password(rig.addr(), "bare", "wrong password here").await;
        assert!(failed(&result));
        absent.push(start.elapsed());
    }
    let (wrong, unknown, absent) = (median(wrong), median(unknown), median(absent));
    for other in [unknown, absent] {
        let ratio = other.as_secs_f64() / wrong.as_secs_f64();
        assert!(
            (0.5..2.0).contains(&ratio),
            "wrong {wrong:?} vs other {other:?}: an attacker could tell them apart"
        );
    }
}

// ---------------------------------------------------------------- limits

#[tokio::test]
async fn attempts_per_handle_are_limited_whether_or_not_the_handle_exists() {
    let limits = Limits {
        password_account_rate: RatePolicy::new(3, Duration::from_secs(3600)),
        ..lenient(true)
    };
    let rig = Rig::start(limits).await;
    let (alice, _) = rig.member("alice").await;
    rig.give_password(alice, SECRET).await;
    for _ in 0..3 {
        let (_s, result) = try_password(rig.addr(), "alice", "wrong password here").await;
        assert!(failed(&result));
    }
    // The fourth attempt has the right password and is refused anyway: the handle is limited.
    let (_s, result) = try_password(rig.addr(), "alice", SECRET).await;
    assert!(failed(&result), "limited handle must not sign in");
    // A different handle is not affected, and an unknown handle is limited the same way.
    let (bob, _) = rig.member("bob").await;
    rig.give_password(bob, SECRET).await;
    succeeds(rig.addr(), "bob", SECRET).await;
    for _ in 0..3 {
        let (_s, result) = try_password(rig.addr(), "ghost", "wrong password here").await;
        assert!(failed(&result));
    }
    // Key login is untouched by any of it.
    let (_carol, key) = rig.member("carol").await;
    by_key(rig.addr(), &key).await;
}

#[tokio::test]
async fn attempts_per_network_are_limited_across_handles() {
    let limits = Limits {
        password_network_rate: RatePolicy::new(3, Duration::from_secs(3600)),
        ..lenient(true)
    };
    let rig = Rig::start(limits).await;
    for name in ["u1", "u2", "u3"] {
        let (id, _) = rig.member(name).await;
        rig.give_password(id, SECRET).await;
    }
    succeeds(rig.addr(), "u1", SECRET).await;
    succeeds(rig.addr(), "u2", SECRET).await;
    succeeds(rig.addr(), "u3", SECRET).await;
    // Everything from this address is now refused, right password or not.
    let (_s, result) = try_password(rig.addr(), "u1", SECRET).await;
    assert!(failed(&result));
}

// ---------------------------------------------------------------- the screen

#[tokio::test]
async fn a_key_session_sets_a_password_that_then_signs_in_and_nothing_readable_is_stored() {
    let rig = Rig::start(lenient(true)).await;
    let (id, key) = rig.member("alice").await;
    let session = by_key(rig.addr(), &key).await;
    let mut channel = at_menu(&session).await;
    let page = to_password_page(&mut channel).await;
    assert!(page.contains("No password is set"), "{page}");
    assert!(page.contains("[s] Set or change password"), "{page}");
    assert!(!page.contains("[c] Clear password"), "{page}");

    send(&mut channel, "s").await;
    // A key session is not asked for a current password.
    let prompt = full(&mut channel).await;
    assert!(prompt.contains("New password"), "{prompt}");
    assert!(!prompt.contains("Current password"), "{prompt}");
    let typed = send(&mut channel, SECRET).await;
    assert!(
        !typed.contains(SECRET),
        "the password must not be echoed: {typed}"
    );
    assert!(typed.contains(&"*".repeat(SECRET.len())), "{typed}");
    send(&mut channel, "\r").await;
    send(&mut channel, SECRET).await;
    let done = send_until(&mut channel, "\r", "Password set.").await;
    assert!(!done.contains(SECRET));
    assert!(done.contains("[c] Clear password"), "{done}");

    // It is stored as an argon2id hash and nowhere in the clear.
    let hash = rig.hash_of(id).await.unwrap();
    assert!(
        hash.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"),
        "{hash}"
    );
    assert_eq!(rig.audit_actions().await, ["password.set"]);
    let audit: Vec<String> =
        sqlx::query_scalar("SELECT detail || '|' || target_ref FROM audit_log")
            .fetch_all(&rig.db)
            .await
            .unwrap();
    assert!(
        audit
            .iter()
            .all(|row| !row.contains(SECRET) && !row.contains("argon2")),
        "{audit:?}"
    );
    sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
        .execute(&rig.db)
        .await
        .unwrap();
    let bytes = std::fs::read(rig.dir.path().join("bbs.db")).unwrap();
    assert!(
        !bytes.windows(SECRET.len()).any(|w| w == SECRET.as_bytes()),
        "the password is in the database file"
    );

    // And it signs in.
    succeeds(rig.addr(), "alice", SECRET).await;
}

#[tokio::test]
async fn a_short_or_mismatched_password_changes_nothing() {
    let rig = Rig::start(lenient(true)).await;
    let (id, key) = rig.member("alice").await;
    let session = by_key(rig.addr(), &key).await;
    let mut channel = at_menu(&session).await;
    to_password_page(&mut channel).await;
    send(&mut channel, "s").await;
    send(&mut channel, "short").await;
    let screen = send(&mut channel, "\r").await;
    assert!(screen.contains("Too short"), "{screen}");
    send(&mut channel, SECRET).await;
    send(&mut channel, "\r").await;
    send(&mut channel, NEXT).await;
    let screen = send(&mut channel, "\r").await;
    assert!(screen.contains("did not match"), "{screen}");
    assert!(rig.hash_of(id).await.is_none());
    assert!(rig.audit_actions().await.is_empty());
}

#[tokio::test]
async fn a_password_session_must_give_the_current_password_to_change_or_clear_it() {
    let rig = Rig::start(lenient(true)).await;
    let (id, _) = rig.member("alice").await;
    rig.give_password(id, SECRET).await;
    let before = rig.hash_of(id).await.unwrap();
    let session = succeeds(rig.addr(), "alice", SECRET).await;
    let mut channel = at_menu(&session).await;
    let page = to_password_page(&mut channel).await;
    assert!(page.contains("A password is set"), "{page}");

    // Change: the current password is asked first; a wrong one changes nothing.
    send(&mut channel, "s").await;
    let prompt = full(&mut channel).await;
    assert!(prompt.contains("Current password"), "{prompt}");
    send(&mut channel, "not my password").await;
    send(&mut channel, "\r").await;
    send(&mut channel, NEXT).await;
    send(&mut channel, "\r").await;
    send(&mut channel, NEXT).await;
    send_until(&mut channel, "\r", "not the current password").await;
    assert_eq!(rig.hash_of(id).await.unwrap(), before);

    // The right current password changes it: the old one stops working, the new one works.
    send(&mut channel, "s").await;
    send(&mut channel, SECRET).await;
    send(&mut channel, "\r").await;
    send(&mut channel, NEXT).await;
    send(&mut channel, "\r").await;
    send(&mut channel, NEXT).await;
    send_until(&mut channel, "\r", "Password set.").await;
    let (_s, old) = try_password(rig.addr(), "alice", SECRET).await;
    assert!(failed(&old));
    succeeds(rig.addr(), "alice", NEXT).await;

    // Clear: also asks, and afterwards only the key signs in.
    send(&mut channel, "c").await;
    let prompt = full(&mut channel).await;
    assert!(prompt.contains("Current password"), "{prompt}");
    send(&mut channel, "wrong wrong wrong").await;
    send_until(&mut channel, "\r", "not the current password").await;
    assert!(rig.hash_of(id).await.is_some());
    send(&mut channel, "c").await;
    send(&mut channel, NEXT).await;
    send_until(&mut channel, "\r", "Password cleared.").await;
    assert!(rig.hash_of(id).await.is_none());
    let (_s, result) = try_password(rig.addr(), "alice", NEXT).await;
    assert!(failed(&result));
    // The fixture's own set, then the screen's change and clear.
    assert_eq!(
        rig.audit_actions().await,
        ["password.set", "password.set", "password.cleared"]
    );
}

#[tokio::test]
async fn a_key_session_clears_after_a_confirmation_without_the_password() {
    let rig = Rig::start(lenient(true)).await;
    let (id, key) = rig.member("alice").await;
    rig.give_password(id, SECRET).await;
    let session = by_key(rig.addr(), &key).await;
    let mut channel = at_menu(&session).await;
    to_password_page(&mut channel).await;
    let ask = send(&mut channel, "c").await;
    assert!(ask.contains("Clear the password?"), "{ask}");
    send(&mut channel, "n").await;
    assert!(rig.hash_of(id).await.is_some());
    send(&mut channel, "c").await;
    send_until(&mut channel, "y", "Password cleared.").await;
    assert!(rig.hash_of(id).await.is_none());
}

#[tokio::test]
async fn the_screen_exists_even_when_login_is_off_and_needs_the_permission() {
    // Passwords can be prepared while password login is off (and are used by telnet later).
    let rig = Rig::start(lenient(false)).await;
    let (_id, key) = rig.member("alice").await;
    let session = by_key(rig.addr(), &key).await;
    let mut channel = at_menu(&session).await;
    send(&mut channel, "s").await;
    let settings = full(&mut channel).await;
    assert!(settings.contains("[s] Password"), "{settings}");

    // Take the permission away: the entry disappears and the key does nothing.
    sqlx::query("DELETE FROM role_permission WHERE permission = ?")
        .bind(Permission::AccountSetPassword.name())
        .execute(&rig.db)
        .await
        .unwrap();
    send(&mut channel, "b").await;
    send(&mut channel, "s").await;
    let settings = full(&mut channel).await;
    assert!(!settings.contains("Password"), "{settings}");
    let after = send(&mut channel, "s").await;
    assert!(!after.contains("No password is set"), "{after}");
}

#[tokio::test]
async fn a_password_session_whose_account_was_suspended_after_the_login_gets_no_shell() {
    let rig = Rig::start(lenient(true)).await;
    let (id, _) = rig.member("alice").await;
    rig.give_password(id, SECRET).await;
    let session = succeeds(rig.addr(), "alice", SECRET).await;
    sqlx::query("UPDATE accounts SET state = 'suspended' WHERE id = ?")
        .bind(id.as_raw())
        .execute(&rig.db)
        .await
        .unwrap();
    let mut channel = session.channel_open_session().await.unwrap();
    channel
        .request_pty(true, "xterm-256color", 80, 24, 0, 0, &[])
        .await
        .unwrap();
    common::expect_reply(&mut channel, true).await;
    channel.request_shell(true).await.unwrap();
    // Refused: not the waiting room, not a menu.
    common::expect_reply(&mut channel, false).await;
}
