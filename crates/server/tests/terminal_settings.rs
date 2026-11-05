//! Terminal settings, the glyph test, and the login and logoff screens over a real russh
//! client: the founder claims, meets the glyph test on the first login, the answer is stored,
//! a reconnect skips the test, and logoff says goodbye. Keys are generated here; the setup
//! token comes from the server's own console output.
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
use russh_bbs_core::access::Fingerprint;
use russh_bbs_core::icons::{ContentIcon, Icon, Tier};
use russh_bbs_core::store::AccountStore;
use russh_bbs_core::terminal_settings::{TerminalSettings, TerminalSettingsStore};
use russh_bbs_server::founder;
use russh_bbs_server::ssh::{HostKey, Limits, ServerConfig, SshServer};
use russh_bbs_store::SqliteStore;
use tempfile::TempDir;
use tokio::time::timeout;

struct Rig {
    server: SshServer,
    store: SqliteStore,
    token: String,
    _dir: TempDir,
}

impl Rig {
    async fn start() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteStore::open(&dir.path().join("bbs.db")).await.unwrap();
        let host_key = HostKey::load_or_create(&dir.path().join("host_key")).unwrap();
        let mut console = Vec::new();
        let gate = founder::start(&store, &mut console).await.unwrap();
        let token = String::from_utf8(console)
            .unwrap()
            .lines()
            .find(|l| l.starts_with("    "))
            .map(|l| l.trim().to_owned())
            .expect("a fresh board prints a token");
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
        let server = SshServer::start_with_founder(
            &config,
            &host_key,
            Arc::new(store.clone()),
            Arc::new(gate),
        )
        .await
        .unwrap();
        Self {
            server,
            store,
            token,
            _dir: dir,
        }
    }

    fn addr(&self) -> SocketAddr {
        self.server.local_addr()
    }

    /// The settings stored for the account behind `key`.
    async fn stored(&self, key: &PrivateKey) -> TerminalSettings {
        let fp =
            Fingerprint::parse(&key.public_key().fingerprint(HashAlg::Sha256).to_string()).unwrap();
        let account = self
            .store
            .account_by_fingerprint(&fp)
            .await
            .unwrap()
            .unwrap();
        self.store.terminal_settings(account.id).await.unwrap()
    }
}

async fn login(addr: SocketAddr, user: &str, key: &PrivateKey) -> Handle<Client> {
    let mut session = common::connect(addr).await;
    let result = session
        .authenticate_publickey(
            &common::user_for(key, user),
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

/// Send `bytes`, then repaint the whole screen with a same-size window change, so the words of
/// the new screen are whole: a diff redraw leaves out cells the previous screen already had in
/// the same place (UI-3: the login screen now has art or large letters, which moves those).
async fn send_repainted(channel: &mut Channel<client::Msg>, bytes: &str) -> (String, bool) {
    channel.data_bytes(bytes.as_bytes().to_vec()).await.unwrap();
    let _ = words(channel).await;
    channel.window_change(80, 24, 0, 0).await.unwrap();
    words(channel).await
}

/// Claim the board as the founder with `key`; returns the open channel and the login screen.
async fn claim(rig: &Rig, key: &PrivateKey) -> (Handle<Client>, Channel<client::Msg>, String) {
    let session = login(rig.addr(), "claim", key).await;
    let mut channel = common::open_shell(&session, 80, 24).await;
    words(&mut channel).await;
    send(&mut channel, &format!("{}\r", rig.token)).await;
    send(&mut channel, &format!("{}\r", "founder password one")).await;
    let (screen, _) = send(&mut channel, &format!("{}\r", "founder password one")).await;
    common::remember(
        key.public_key().fingerprint(russh::keys::HashAlg::Sha256),
        "founder",
    );
    (session, channel, screen)
}

async fn reconnect(rig: &Rig, key: &PrivateKey) -> (Handle<Client>, Channel<client::Msg>, String) {
    let session = login(rig.addr(), "anyone", key).await;
    let mut channel = common::open_shell(&session, 80, 24).await;
    let (screen, _) = words(&mut channel).await;
    (session, channel, screen)
}

/// The whole current screen. ratatui sends only changed cells, so an incremental read can miss
/// letters that did not change; a window-change makes the server clear and redraw everything.
async fn full(channel: &mut Channel<client::Msg>) -> String {
    channel.window_change(80, 24, 0, 0).await.unwrap();
    words(channel).await.0
}

const QUESTION: &str = "Do these look like icons?";

#[tokio::test]
async fn first_login_runs_the_glyph_test_and_the_answer_is_stored() {
    let rig = Rig::start().await;
    let key = new_client_key();

    // The claim ends at the login screen of a first login on the plain defaults.
    let (_s, mut channel, login_screen) = claim(&rig, &key).await;
    assert!(
        login_screen.contains("Signed in as founder."),
        "{login_screen}"
    );
    assert!(
        login_screen.contains("This is your first login."),
        "{login_screen}"
    );
    assert!(
        login_screen.contains("icon style not chosen yet"),
        "{login_screen}"
    );
    assert!(rig.stored(&key).await.needs_glyph_test());

    // Any key moves on to the glyph test, which draws the registry's Nerd Font glyphs.
    let (test_screen, _) = send_repainted(&mut channel, "\r").await;
    assert!(test_screen.contains(QUESTION), "{test_screen}");
    assert!(test_screen.contains("Style: Nerd Font"), "{test_screen}");
    let board = Icon::from(ContentIcon::Board).glyph(Tier::NerdFont);
    assert!(
        test_screen.contains(board),
        "registry glyph on screen: {test_screen}"
    );

    // Answer yes: the tier is stored and the placeholder home shows.
    let (home, _) = send(&mut channel, "y").await;
    assert!(home.contains("Icons set to Nerd Font."), "{home}");
    assert!(home.contains("[s] Terminal settings"), "{home}");
    assert_eq!(rig.stored(&key).await.chosen_tier(), Some(Tier::NerdFont));

    // Logoff says goodbye and ends the session.
    let (bye, closed) = send(&mut channel, "l").await;
    assert!(bye.contains("Goodbye, founder."), "{bye}");
    assert!(closed, "the session ends after logoff");
    rig.server.shutdown().await;
}

#[tokio::test]
async fn a_reconnect_skips_the_glyph_test_and_shows_the_stored_tier() {
    let rig = Rig::start().await;
    let key = new_client_key();
    let (_s, mut channel, _) = claim(&rig, &key).await;
    send(&mut channel, "\r").await;
    send(&mut channel, "ny").await;
    assert_eq!(rig.stored(&key).await.chosen_tier(), Some(Tier::Unicode));
    send(&mut channel, "l").await;

    let (_s2, mut again, screen) = reconnect(&rig, &key).await;
    assert!(screen.contains("Signed in as founder."), "{screen}");
    assert!(screen.contains("Last login:"), "{screen}");
    assert!(
        screen.contains("Terminal: Unicode icons, color on, pause on"),
        "{screen}"
    );
    send(&mut again, "x").await;
    let next = full(&mut again).await;
    assert!(
        !next.contains(QUESTION),
        "no glyph test the second time: {next}"
    );
    assert!(next.contains("Main menu"), "{next}");
    assert!(next.contains("not built yet"), "{next}");
    rig.server.shutdown().await;
}

#[tokio::test]
async fn skipping_the_glyph_test_stores_ascii_without_color() {
    let rig = Rig::start().await;
    let key = new_client_key();
    let (_s, mut channel, _) = claim(&rig, &key).await;
    send(&mut channel, "\r").await;
    // A lone Escape byte is the skip key.
    let (home, _) = send(&mut channel, "\u{1b}").await;
    assert!(home.contains("plain ASCII icons and no color"), "{home}");
    let stored = rig.stored(&key).await;
    assert_eq!(stored.chosen_tier(), Some(Tier::Ascii));
    assert!(!stored.use_color());
    send(&mut channel, "l").await;

    let (_s2, _c, screen) = reconnect(&rig, &key).await;
    assert!(
        screen.contains("Terminal: ASCII icons, color off"),
        "{screen}"
    );
    rig.server.shutdown().await;
}

#[tokio::test]
async fn answering_no_to_every_style_stores_ascii_without_color() {
    let rig = Rig::start().await;
    let key = new_client_key();
    let (_s, mut channel, _) = claim(&rig, &key).await;
    send(&mut channel, "\r").await;
    send(&mut channel, "n").await;
    let second = full(&mut channel).await;
    assert!(second.contains("Style: Unicode"), "{second}");
    send(&mut channel, "n").await;
    let stored = rig.stored(&key).await;
    assert_eq!(stored.chosen_tier(), Some(Tier::Ascii));
    assert!(!stored.use_color());
    rig.server.shutdown().await;
}

#[tokio::test]
async fn leaving_before_the_glyph_test_is_answered_asks_again_next_time() {
    let rig = Rig::start().await;
    let key = new_client_key();
    let (_s, mut channel, _) = claim(&rig, &key).await;
    let (bye, closed) = send(&mut channel, "\u{3}").await;
    assert!(bye.contains("Goodbye, founder."), "{bye}");
    assert!(closed);
    assert!(rig.stored(&key).await.needs_glyph_test());

    let (_s2, mut again, _) = reconnect(&rig, &key).await;
    let (screen, _) = send_repainted(&mut again, "x").await;
    assert!(screen.contains(QUESTION), "{screen}");
    rig.server.shutdown().await;
}

#[tokio::test]
async fn settings_changes_are_stored_and_the_test_can_be_run_again() {
    let rig = Rig::start().await;
    let key = new_client_key();
    let (_s, mut channel, _) = claim(&rig, &key).await;
    send(&mut channel, "\r").await;
    send(&mut channel, "y").await;

    send(&mut channel, "s").await;
    let menu = full(&mut channel).await;
    assert!(menu.contains("Terminal settings"), "{menu}");
    assert!(menu.contains("Width: automatic"), "{menu}");
    send(&mut channel, "c").await; // color off
    send(&mut channel, "p").await; // pause off
    send(&mut channel, "w100\r").await;
    send(&mut channel, "h30\r").await;
    let menu = full(&mut channel).await;
    assert!(menu.contains("Height: 30"), "{menu}");
    assert!(menu.contains("Width: 100"), "{menu}");
    assert!(menu.contains("Color: off"), "{menu}");
    assert!(menu.contains("Pause at screen end: off"), "{menu}");
    let stored = rig.stored(&key).await;
    assert_eq!(stored.width(), Some(100));
    assert_eq!(stored.height(), Some(30));
    assert!(!stored.color());
    assert!(!stored.pause());
    assert_eq!(stored.chosen_tier(), Some(Tier::NerdFont));

    // Out of range is refused in words and stores nothing.
    send(&mut channel, "w9999\r").await;
    let menu = full(&mut channel).await;
    assert!(
        menu.contains("Problem: Width must be from 20 to 500."),
        "{menu}"
    );
    assert_eq!(rig.stored(&key).await.width(), Some(100));

    // Reachable later: run the glyph test again and change the answer.
    send(&mut channel, "i").await;
    let test = full(&mut channel).await;
    assert!(test.contains(QUESTION), "{test}");
    assert!(test.contains("Esc: cancel"), "{test}");
    send(&mut channel, "ny").await;
    assert_eq!(rig.stored(&key).await.chosen_tier(), Some(Tier::Unicode));
    rig.server.shutdown().await;
}

#[tokio::test]
async fn resizing_during_the_session_keeps_it_working() {
    let rig = Rig::start().await;
    let key = new_client_key();
    let (_s, mut channel, _) = claim(&rig, &key).await;
    send(&mut channel, "\r").await;
    send(&mut channel, "y").await;
    for (cols, rows) in [(132, 43), (1, 1), (40, 10), (0, 0), (600, 300), (80, 24)] {
        channel.window_change(cols, rows, 0, 0).await.unwrap();
        words(&mut channel).await;
    }
    let (menu, _) = send(&mut channel, "s").await;
    assert!(
        menu.contains("Terminal settings"),
        "still alive after resizes: {menu}"
    );
    // A width override is applied on top of the client's size and survives a resize.
    send(&mut channel, "w60\r").await;
    channel.window_change(100, 30, 0, 0).await.unwrap();
    words(&mut channel).await;
    let (bye, closed) = send(&mut channel, "b").await;
    assert!(!closed, "{bye}");
    rig.server.shutdown().await;
}

#[tokio::test]
async fn an_unknown_key_still_sees_only_the_waiting_room() {
    let rig = Rig::start().await;
    let stranger = new_client_key();
    let session = login(rig.addr(), "anyone", &stranger).await;
    let mut channel = common::open_shell(&session, 80, 24).await;
    let (screen, _) = words(&mut channel).await;
    assert!(screen.contains("Waiting room"), "{screen}");
    for leaked in [
        "Signed in",
        "Terminal settings",
        "Last login",
        "founder",
        QUESTION,
    ] {
        assert!(!screen.contains(leaked), "{leaked:?} leaked: {screen}");
    }
    // Keys that would drive the member screens do nothing for a stranger.
    let (after, _) = send(&mut channel, "slyc").await;
    for leaked in ["Terminal settings", QUESTION, "Goodbye"] {
        assert!(!after.contains(leaked), "{leaked:?} leaked: {after}");
    }
    rig.server.shutdown().await;
}
