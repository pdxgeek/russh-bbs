//! Resource limits on signed-in SSH sessions (M2-H, D62) over a real russh client against the
//! real SQLite store: concurrent sessions, calls per day and minutes per day, each refused with
//! an honest message that names the limit and when it resets. Members are created through the
//! store (the invite flow is a later slice) with keys generated here.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unused_async_trait_impl,
    missing_docs
)]

mod common;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use common::{Client, fast_limits, new_client_key};
use russh::client::{self, AuthResult, Handle};
use russh::keys::{HashAlg, PrivateKey, PrivateKeyWithHashAlg};
use russh::{Channel, ChannelMsg};
use russh_bbs_core::access::{Fingerprint, Slug};
use russh_bbs_core::limits::{Clock, Day, Limit, LimitKind, LimitStore, SystemClock};
use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::store::{Account, AccountState, AccountStore, NewAccount};
use russh_bbs_server::ssh::{HostKey, Limits, ServerConfig, SshServer};
use russh_bbs_store::SqliteStore;
use tempfile::TempDir;
use tokio::time::timeout;

struct Rig {
    server: SshServer,
    store: SqliteStore,
    _dir: TempDir,
}

impl Rig {
    async fn start() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteStore::open(&dir.path().join("bbs.db")).await.unwrap();
        let host_key = HostKey::load_or_create(&dir.path().join("host_key")).unwrap();
        let limits = Limits {
            ip_rate: russh_bbs_core::waiting_room::RatePolicy::new(1_000, Duration::from_millis(1)),
            key_rate: russh_bbs_core::waiting_room::RatePolicy::new(
                1_000,
                Duration::from_millis(1),
            ),
            ..fast_limits()
        };
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
            _dir: dir,
        }
    }

    fn addr(&self) -> SocketAddr {
        self.server.local_addr()
    }

    /// An active member holding the built-in `member` role, with a key.
    async fn member(&self, name: &str) -> (Account, PrivateKey) {
        let account = self
            .store
            .create_account(NewAccount {
                username: sanitize_username(name).unwrap(),
                role: Slug::MEMBER,
                state: AccountState::Active,
            })
            .await
            .unwrap();
        let key = new_client_key();
        let fp =
            Fingerprint::parse(&key.public_key().fingerprint(HashAlg::Sha256).to_string()).unwrap();
        self.store
            .attach_fingerprint(account.id, &fp)
            .await
            .unwrap();
        (account, key)
    }

    async fn override_limit(&self, account: &Account, kind: LimitKind, limit: Limit) {
        self.store
            .set_member_limit(account.id, kind, Some(limit))
            .await
            .unwrap();
    }
}

async fn login(addr: SocketAddr, user: &str, key: &PrivateKey) -> Handle<Client> {
    let mut session = common::connect(addr).await;
    let result = session
        .authenticate_publickey(
            user,
            PrivateKeyWithHashAlg::new(Arc::new(key.clone()), None),
        )
        .await
        .unwrap();
    assert!(matches!(result, AuthResult::Success), "{result:?}");
    session
}

/// Read until the channel is quiet; returns the printable text as words (escape sequences and
/// box characters dropped) and whether the channel closed.
async fn words(channel: &mut Channel<client::Msg>) -> (String, bool) {
    let mut raw = String::new();
    let mut closed = false;
    while let Ok(msg) = timeout(Duration::from_millis(200), channel.wait()).await {
        match msg {
            Some(ChannelMsg::Data { data }) => raw.push_str(&String::from_utf8_lossy(&data)),
            Some(ChannelMsg::Close | ChannelMsg::Eof) | None => {
                closed = true;
                break;
            }
            Some(_) => {}
        }
    }
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
    (out.split_whitespace().collect::<Vec<_>>().join(" "), closed)
}

async fn send(channel: &mut Channel<client::Msg>, bytes: &str) -> (String, bool) {
    channel.data_bytes(bytes.as_bytes().to_vec()).await.unwrap();
    words(channel).await
}

/// Connect and read the first screen: a signed-in member sees "Signed in as", a refused one the
/// limit message. Returns the screen and whether the server already closed the channel.
async fn connect_as(
    rig: &Rig,
    key: &PrivateKey,
) -> (Handle<Client>, Channel<client::Msg>, String, bool) {
    let session = login(rig.addr(), "anyone", key).await;
    let mut channel = common::open_shell(&session, 80, 24).await;
    let (screen, closed) = words(&mut channel).await;
    (session, channel, screen, closed)
}

#[tokio::test]
async fn calls_per_day_refuse_the_next_sign_in_with_the_reset_time() {
    let rig = Rig::start().await;
    let (account, key) = rig.member("ann").await;
    rig.override_limit(&account, LimitKind::CallsPerDay, Limit::AtMost(1))
        .await;

    let (first, first_channel, screen, _) = connect_as(&rig, &key).await;
    assert!(screen.contains("Signed in as ann"), "{screen}");
    drop((first_channel, first));

    let (_session, mut channel, screen, closed) = connect_as(&rig, &key).await;
    assert!(screen.contains("Limit reached"), "{screen}");
    assert!(
        screen.contains("You have used your 1 sign in for today."),
        "{screen}"
    );
    assert!(screen.contains("resets at 00:00 UTC"), "{screen}");
    assert!(!screen.contains("Signed in as"), "{screen}");
    // The message ends the session on its own.
    assert!(
        closed || words(&mut channel).await.1,
        "the session should end"
    );
    rig.server.shutdown().await;
}

#[tokio::test]
async fn concurrent_sessions_are_limited_and_the_slot_comes_back() {
    let rig = Rig::start().await;
    let (account, key) = rig.member("ann").await;
    rig.override_limit(&account, LimitKind::MaxSessions, Limit::AtMost(1))
        .await;

    let (first, first_channel, screen, _) = connect_as(&rig, &key).await;
    assert!(screen.contains("Signed in as ann"), "{screen}");

    let (_s, _c, screen, _) = connect_as(&rig, &key).await;
    assert!(
        screen.contains("may have 1 session open at once"),
        "{screen}"
    );
    assert!(screen.contains("no timed reset"), "{screen}");

    // Another member is not affected by ann's limit.
    let (_, other_key) = rig.member("bob").await;
    let (_bs, _bc, screen, _) = connect_as(&rig, &other_key).await;
    assert!(screen.contains("Signed in as bob"), "{screen}");

    // When the first session ends its slot is free again.
    drop((first_channel, first));
    let mut tries = 0;
    loop {
        let (_s, _c, screen, _) = connect_as(&rig, &key).await;
        if screen.contains("Signed in as ann") {
            break;
        }
        tries += 1;
        assert!(tries < 20, "the slot never came back: {screen}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    rig.server.shutdown().await;
}

#[tokio::test]
async fn a_day_with_no_minutes_left_refuses_the_sign_in() {
    let rig = Rig::start().await;
    let (account, key) = rig.member("ann").await;
    rig.override_limit(&account, LimitKind::MinutesPerDay, Limit::AtMost(1))
        .await;
    let today = Day::of(SystemClock.now());
    rig.store
        .add_usage(account.id, LimitKind::MinutesPerDay, today, 60, None)
        .await
        .unwrap();
    let (_s, _c, screen, _) = connect_as(&rig, &key).await;
    assert!(
        screen.contains("You have used your 1 minute for today."),
        "{screen}"
    );
    assert!(screen.contains("resets at 00:00 UTC"), "{screen}");
    // The refused attempt did not use up a call.
    assert_eq!(
        rig.store
            .usage(account.id, LimitKind::CallsPerDay, today)
            .await
            .unwrap(),
        0
    );
    rig.server.shutdown().await;
}

#[tokio::test]
async fn minutes_run_out_during_a_session_and_the_member_is_logged_off() {
    let rig = Rig::start().await;
    let (account, key) = rig.member("ann").await;
    rig.override_limit(&account, LimitKind::MinutesPerDay, Limit::AtMost(1))
        .await;
    // One second short of the minute: the session starts, then runs out a moment later.
    let today = Day::of(SystemClock.now());
    rig.store
        .add_usage(account.id, LimitKind::MinutesPerDay, today, 59, None)
        .await
        .unwrap();
    let (_session, mut channel, screen, _) = connect_as(&rig, &key).await;
    assert!(screen.contains("Signed in as ann"), "{screen}");

    tokio::time::sleep(Duration::from_millis(2_200)).await;
    // The next action (a key) is where the meter speaks.
    let (screen, mut closed) = send(&mut channel, "x").await;
    assert!(screen.contains("Limit reached"), "{screen}");
    assert!(
        screen.contains("You have used your 1 minute for today."),
        "{screen}"
    );
    assert!(screen.contains("You are being signed off."), "{screen}");
    if !closed {
        closed = words(&mut channel).await.1;
    }
    assert!(closed, "the session ends after the message");

    // The time used is written down when the session ends, so a restart would still know it.
    let mut used = 0;
    for _ in 0..40 {
        used = rig
            .store
            .usage(account.id, LimitKind::MinutesPerDay, today)
            .await
            .unwrap();
        if used >= 60 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(used >= 60, "{used}");
    rig.server.shutdown().await;
}

#[tokio::test]
async fn the_waiting_room_is_not_limited_by_a_members_limits() {
    let rig = Rig::start().await;
    let (account, _key) = rig.member("ann").await;
    rig.override_limit(&account, LimitKind::MaxSessions, Limit::AtMost(0))
        .await;
    let stranger = new_client_key();
    let (_s, _c, screen, _) = connect_as(&rig, &stranger).await;
    assert!(screen.contains("Waiting room"), "{screen}");
    rig.server.shutdown().await;
}
