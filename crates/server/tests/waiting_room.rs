//! The waiting room over a real SSH client: what an unknown key sees in each registration
//! mode, that it learns nothing about the board, and that the per-key and per-network limits
//! throttle it. Everything uses a temporary database and keys generated here.
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

use common::{Client, fast_limits, new_client_key, open_shell};
use russh::Channel;
use russh::ChannelMsg;
use russh::client::{self, AuthResult, Handle, Msg};
use russh::keys::{PrivateKey, PrivateKeyWithHashAlg};
use russh_bbs_core::access::Slug;
use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::store::{
    AccountState, AccountStore, NewAccount, RegistrationMode, SettingKey, SettingsStore,
    set_registration_mode,
};
use russh_bbs_core::waiting_room::RatePolicy;
use russh_bbs_server::ssh::{HostKey, Limits, ServerConfig, SshServer};
use russh_bbs_store::SqliteStore;
use tempfile::TempDir;
use tokio::time::timeout;

const BANNER: &str = "Welcome to the Test Lounge";
const RULES: &str = "Rule one: be excellent";

struct Rig {
    server: SshServer,
    store: SqliteStore,
    _dir: TempDir,
}

impl Rig {
    async fn start(mode: RegistrationMode, limits: Limits) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteStore::open(&dir.path().join("bbs.db")).await.unwrap();
        set_registration_mode(&store, mode).await.unwrap();
        store
            .set_setting(SettingKey::WaitingRoomBanner, BANNER)
            .await
            .unwrap();
        store
            .set_setting(SettingKey::WaitingRoomRules, RULES)
            .await
            .unwrap();
        // Members and a founder flag exist; an unknown key must learn none of it.
        for name in ["zorgon_member", "qwerty_sysop"] {
            store
                .create_account(NewAccount {
                    username: sanitize_username(name).unwrap(),
                    role: Slug::MEMBER,
                    state: AccountState::Active,
                })
                .await
                .unwrap();
        }
        store
            .set_setting(SettingKey::FounderClaimed, "true")
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
            _dir: dir,
        }
    }

    fn addr(&self) -> SocketAddr {
        self.server.local_addr()
    }

    async fn stop(self) {
        self.server.shutdown().await;
        self.store.close().await;
    }
}

/// Limits generous enough that the throttles do not interfere with a walkthrough.
fn relaxed() -> Limits {
    Limits {
        ip_rate: RatePolicy::new(1_000, Duration::from_millis(1)),
        key_rate: RatePolicy::new(1_000, Duration::from_millis(1)),
        ..fast_limits()
    }
}

async fn try_login(addr: SocketAddr, key: &PrivateKey) -> Option<(Handle<Client>, AuthResult)> {
    let config = Arc::new(client::Config::default());
    let mut session = timeout(common::WAIT, client::connect(config, addr, Client))
        .await
        .ok()?
        .ok()?;
    let result = timeout(
        common::WAIT,
        session.authenticate_publickey(
            &common::user_for(key, "anyone"),
            PrivateKeyWithHashAlg::new(Arc::new(key.clone()), None),
        ),
    )
    .await
    .ok()?
    .ok()?;
    Some((session, result))
}

/// Replay the terminal byte stream onto a blank screen and return the rows as text, the way a
/// person would see them. Only what ratatui emits is understood: printable text, cursor
/// positioning (`H`) and cursor-forward (`C`); every other escape sequence is ignored.
fn strip_ansi(raw: &str) -> String {
    let mut rows: Vec<Vec<char>> = Vec::new();
    let (mut row, mut col) = (0_usize, 0_usize);
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\u{1b}' if chars.peek() == Some(&'[') => {
                chars.next();
                let mut params = String::new();
                for next in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&next) {
                        let numbers: Vec<usize> =
                            params.split(';').map(|p| p.parse().unwrap_or(1)).collect();
                        match next {
                            'H' => {
                                row = numbers.first().copied().unwrap_or(1).saturating_sub(1);
                                col = numbers.get(1).copied().unwrap_or(1).saturating_sub(1);
                            }
                            'C' => col += numbers.first().copied().unwrap_or(1),
                            _ => {}
                        }
                        break;
                    }
                    params.push(next);
                }
            }
            '\u{1b}' => {
                chars.next();
            }
            c if c.is_control() => {}
            c => {
                while rows.len() <= row {
                    rows.push(Vec::new());
                }
                let line = &mut rows[row];
                if line.len() <= col {
                    line.resize(col + 1, ' ');
                }
                line[col] = c;
                col += 1;
            }
        }
    }
    rows.iter()
        .map(|r| r.iter().collect::<String>().trim_end().to_owned())
        .collect::<Vec<_>>()
        .join("\n")
}

/// A screen as one line of words, so a message that wrapped across rows can be matched.
fn flat(screen: &str) -> String {
    screen
        .replace(['│', '┌', '┐', '└', '┘', '─'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Read until the channel has been quiet for a moment, returning the text seen.
async fn drain(channel: &mut Channel<Msg>) -> String {
    let mut seen = String::new();
    while let Ok(msg) = timeout(Duration::from_millis(120), channel.wait()).await {
        match msg {
            Some(ChannelMsg::Data { data }) => seen.push_str(&String::from_utf8_lossy(&data)),
            Some(_) => {}
            None => break,
        }
    }
    seen
}

/// Send input, then force a full redraw and return the whole screen as text.
async fn press(channel: &mut Channel<Msg>, input: &[u8]) -> String {
    channel.data_bytes(input.to_vec()).await.unwrap();
    drain(channel).await;
    channel.window_change(80, 24, 0, 0).await.unwrap();
    strip_ansi(&drain(channel).await)
}

async fn type_line(channel: &mut Channel<Msg>, text: &str) -> String {
    let mut bytes = text.as_bytes().to_vec();
    bytes.push(b'\r');
    press(channel, &bytes).await
}

async fn enter_room(rig: &Rig) -> (Handle<Client>, Channel<Msg>, String) {
    let (session, result) = try_login(rig.addr(), &new_client_key()).await.unwrap();
    assert!(matches!(result, AuthResult::Success), "{result:?}");
    let mut channel = open_shell(&session, 80, 24).await;
    drain(&mut channel).await;
    channel.window_change(80, 24, 0, 0).await.unwrap();
    let home = strip_ansi(&drain(&mut channel).await);
    (session, channel, home)
}

#[tokio::test]
async fn an_unknown_key_walks_every_invite_mode_screen() {
    let rig = Rig::start(RegistrationMode::Invite, relaxed()).await;
    let (_session, mut channel, home) = enter_room(&rig).await;

    assert!(home.contains(BANNER), "sysop banner shown: {home}");
    assert!(home.contains("Read the rules"));
    assert!(home.contains("Request an invite"));
    assert!(home.contains("Redeem an invite code"));
    assert!(!home.contains("Apply for membership") && !home.contains("Create an account"));

    let rules = press(&mut channel, b"r").await;
    assert!(
        rules.contains("House rules") && rules.contains(RULES),
        "{rules}"
    );
    assert!(
        rules.contains("server can read everything"),
        "privacy note: {rules}"
    );
    press(&mut channel, b"\x1b").await; // Escape: back home

    let request = press(&mut channel, b"i").await;
    assert!(request.contains("Request an invite"), "{request}");
    type_line(&mut channel, "newcomer").await;
    type_line(&mut channel, "a friend of a friend").await;
    type_line(&mut channel, "a long join password").await;
    let sent = type_line(&mut channel, "a long join password").await;
    let sent = flat(&sent);
    assert!(
        sent.contains("Thank you. If your request is kept"),
        "{sent}"
    );
    assert!(sent.contains("Nobody is notified"), "{sent}");
    press(&mut channel, b"\x1b").await;

    let redeem = press(&mut channel, b"c").await;
    assert!(redeem.contains("Invite code"), "{redeem}");
    type_line(&mut channel, "ABC-123").await;
    type_line(&mut channel, "newcomer").await;
    type_line(&mut channel, "a long join password").await;
    let result = type_line(&mut channel, "a long join password").await;
    let result = flat(&result);
    assert!(result.contains("That invite could not be used"), "{result}");

    // The same reply, word for word, for a name that belongs to a member: the answer to a
    // refused redemption does not depend on who is on the board.
    press(&mut channel, b"\x1b").await;
    press(&mut channel, b"c").await;
    type_line(&mut channel, "ABC-123").await;
    type_line(&mut channel, "zorgon_member").await;
    type_line(&mut channel, "a long join password").await;
    let again = type_line(&mut channel, "a long join password").await;
    let again = flat(&again);
    assert_eq!(again, result, "a member's handle changed the reply");

    rig.stop().await;
}

#[tokio::test]
async fn switching_the_registration_mode_changes_the_screen_set() {
    let rig = Rig::start(RegistrationMode::Invite, relaxed()).await;

    set_registration_mode(&rig.store, RegistrationMode::Apply)
        .await
        .unwrap();
    let (_s, mut channel, home) = enter_room(&rig).await;
    assert!(home.contains("Apply for membership"), "{home}");
    assert!(!home.contains("Request an invite") && !home.contains("Create an account"));
    let form = press(&mut channel, b"a").await;
    assert!(form.contains("Apply for membership"));
    type_line(&mut channel, "applicant").await;
    type_line(&mut channel, "I like boards").await;
    type_line(&mut channel, "a long join password").await;
    let result = type_line(&mut channel, "a long join password").await;
    let result = flat(&result);
    // M2-G: the application form is live and answers with one fixed text.
    assert!(
        result.contains("Thank you. If your application is kept"),
        "{result}"
    );
    // The invite-mode keys do nothing here.
    press(&mut channel, b"\x1b").await;
    let still_home = press(&mut channel, b"i").await;
    assert!(still_home.contains(BANNER), "{still_home}");

    set_registration_mode(&rig.store, RegistrationMode::Open)
        .await
        .unwrap();
    let (_s2, mut channel2, home) = enter_room(&rig).await;
    assert!(home.contains("Create an account"), "{home}");
    assert!(!home.contains("Request an invite") && !home.contains("Apply for membership"));
    press(&mut channel2, b"n").await;
    type_line(&mut channel2, "freshname").await;
    type_line(&mut channel2, "a long join password").await;
    let result = type_line(&mut channel2, "a long join password").await;
    // M2-G: the create-account form is live; a stranger's handle makes a member at once.
    assert!(flat(&result).contains("Signed in as freshname"), "{result}");

    rig.stop().await;
}

#[tokio::test]
async fn a_corrupt_mode_setting_fails_closed_to_invite_screens() {
    let rig = Rig::start(RegistrationMode::Open, relaxed()).await;
    rig.store
        .set_setting(SettingKey::RegistrationMode, "wide-open")
        .await
        .unwrap();
    let (_s, _channel, home) = enter_room(&rig).await;
    assert!(home.contains("Request an invite"), "{home}");
    assert!(!home.contains("Create an account"));
    rig.stop().await;
}

#[tokio::test]
async fn an_unknown_key_learns_nothing_beyond_the_waiting_room() {
    for mode in [
        RegistrationMode::Invite,
        RegistrationMode::Apply,
        RegistrationMode::Open,
    ] {
        let rig = Rig::start(mode, relaxed()).await;
        let (_s, mut channel, home) = enter_room(&rig).await;
        let mut seen = home;
        // Visit every page the mode offers, submitting nothing and something.
        for key in *b"rican" {
            seen.push_str(&press(&mut channel, &[key]).await);
            seen.push_str(&type_line(&mut channel, "zorgon_member").await);
            seen.push_str(&press(&mut channel, b"\x1b").await);
            seen.push_str(&press(&mut channel, b"\x1b").await);
        }
        let lower = seen.to_lowercase();
        for secret in [
            "zorgon",
            "qwerty",
            "founder",
            "registration_mode",
            "claimed",
        ] {
            // The visitor typed `zorgon_member` themselves; it may be echoed back in their own
            // form, but never confirmed. Only the other strings must be wholly absent.
            if secret != "zorgon" {
                assert!(!lower.contains(secret), "{mode:?} leaks {secret:?}");
            }
        }
        for word in [
            "online",
            "last caller",
            "member list",
            "statistics",
            "users:",
        ] {
            assert!(!lower.contains(word), "{mode:?} leaks {word:?}");
        }
        assert!(!lower.contains("qwerty_sysop"));
        rig.stop().await;
    }
}

#[tokio::test]
async fn q_on_the_home_page_leaves_cleanly() {
    let rig = Rig::start(RegistrationMode::Invite, relaxed()).await;
    let (_session, mut channel, _home) = enter_room(&rig).await;
    channel.data_bytes(&b"q"[..]).await.unwrap();
    let tail = drain(&mut channel).await;
    assert!(tail.contains("\x1b[?1049l"), "terminal restored: {tail:?}");
    rig.stop().await;
}

#[tokio::test]
async fn too_many_form_submissions_end_the_session() {
    let limits = Limits {
        max_form_submissions: 2,
        ..relaxed()
    };
    let rig = Rig::start(RegistrationMode::Open, limits).await;
    let (_session, mut channel, _home) = enter_room(&rig).await;
    press(&mut channel, b"n").await;
    // The create form has three fields now (handle, password, repeat): three Enters submit it,
    // so the third submission is the ninth Enter. Everything the server says is kept.
    let mut tail = String::new();
    for _ in 0..9 {
        if channel.data_bytes(&b"\r"[..]).await.is_err() {
            break;
        }
        tail.push_str(&drain(&mut channel).await);
    }
    assert!(
        flat(&strip_ansi(&tail)).contains("Too many attempts"),
        "{tail:?}"
    );
    assert!(tail.contains("\x1b[?1049l"), "session ended: {tail:?}");
    rig.stop().await;
}

#[tokio::test]
async fn a_key_that_authenticates_too_often_is_throttled() {
    let limits = Limits {
        key_rate: RatePolicy::new(2, Duration::from_secs(3600)),
        ..relaxed()
    };
    let rig = Rig::start(RegistrationMode::Invite, limits).await;
    let key = new_client_key();
    for attempt in 1..=2 {
        let (_s, result) = try_login(rig.addr(), &key).await.unwrap();
        assert!(matches!(result, AuthResult::Success), "attempt {attempt}");
    }
    let (_s, result) = try_login(rig.addr(), &key).await.unwrap();
    assert!(
        matches!(result, AuthResult::Failure { .. }),
        "third attempt is throttled: {result:?}"
    );
    // Another key is unaffected.
    let (_s, other) = try_login(rig.addr(), &new_client_key()).await.unwrap();
    assert!(matches!(other, AuthResult::Success));
    rig.stop().await;
}

#[tokio::test]
async fn a_network_that_connects_too_often_is_dropped_at_accept() {
    let limits = Limits {
        ip_rate: RatePolicy::new(3, Duration::from_secs(3600)),
        ..relaxed()
    };
    let rig = Rig::start(RegistrationMode::Invite, limits).await;
    let mut kept = Vec::new();
    for attempt in 1..=3 {
        let (session, result) = try_login(rig.addr(), &new_client_key()).await.unwrap();
        assert!(matches!(result, AuthResult::Success), "attempt {attempt}");
        kept.push(session);
    }
    // The fourth connection from 127.0.0.1 is closed before the SSH handshake completes.
    assert!(
        try_login(rig.addr(), &new_client_key()).await.is_none(),
        "fourth connection should be dropped"
    );
    drop(kept);
    rig.stop().await;
}

/// M1-D: the founder claim entry (`ssh claim@host`) on a board that is already claimed (the rig
/// sets `founder_claimed`) shows a prompt that names nothing, and a guess gets the one generic
/// failure. The unclaimed state is covered in `tests/founder.rs`, which also asserts the two
/// prompts are word for word identical.
#[tokio::test]
async fn the_claim_entry_on_a_claimed_board_reveals_nothing() {
    for mode in [
        RegistrationMode::Invite,
        RegistrationMode::Apply,
        RegistrationMode::Open,
    ] {
        let rig = Rig::start(mode, relaxed()).await;
        let key = new_client_key();
        let config = Arc::new(client::Config::default());
        let mut session = client::connect(config, rig.addr(), Client).await.unwrap();
        let result = session
            .authenticate_publickey("claim", PrivateKeyWithHashAlg::new(Arc::new(key), None))
            .await
            .unwrap();
        assert!(matches!(result, AuthResult::Success));
        let mut channel = open_shell(&session, 80, 24).await;
        drain(&mut channel).await;
        channel.window_change(80, 24, 0, 0).await.unwrap();
        let prompt = strip_ansi(&drain(&mut channel).await);
        type_line(&mut channel, "AAAA-BBBB-CCCC-DDDD").await;
        type_line(&mut channel, "a long join password").await;
        let after = type_line(&mut channel, "a long join password").await;
        let lower = format!("{prompt}\n{after}").to_lowercase();
        for secret in [
            "zorgon",
            "qwerty",
            "founder",
            "claimed",
            "registration",
            "already",
            "invalid",
            "expired",
            "online",
            "member list",
        ] {
            assert!(
                !lower.contains(secret),
                "{mode:?} leaks {secret:?}: {lower}"
            );
        }
        assert!(
            flat(&after).contains("That token was not accepted."),
            "{after}"
        );
        rig.stop().await;
    }
}
