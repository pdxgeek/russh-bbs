//! Layout and theme switching over a real russh client (M3-A): the founder claims, answers the
//! icon check, switches layout and theme from the settings screen without reconnecting, drives
//! the lightbar with the arrow keys and Enter, and finds the choice stored on the next login.
//! Keys are generated here; the setup token comes from the server's own console.
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
use russh::keys::{PrivateKey, PrivateKeyWithHashAlg};
use russh::{Channel, ChannelMsg};
use russh_bbs_server::founder;
use russh_bbs_server::ssh::{HostKey, Limits, ServerConfig, SshServer};
use russh_bbs_store::SqliteStore;
use tempfile::TempDir;
use tokio::time::timeout;

struct Rig {
    server: SshServer,
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
            token,
            _dir: dir,
        }
    }

    fn addr(&self) -> SocketAddr {
        self.server.local_addr()
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

/// The whole current screen. ratatui sends only changed cells, so an incremental read can miss
/// letters that did not change; a window-change makes the server clear and redraw everything.
async fn full(channel: &mut Channel<client::Msg>) -> String {
    channel.window_change(80, 24, 0, 0).await.unwrap();
    words(channel).await.0
}

/// Claim, answer the icon check with "yes" and return the channel at the main menu.
async fn at_main_menu(rig: &Rig, key: &PrivateKey) -> (Handle<Client>, Channel<client::Msg>) {
    let (session, mut channel, _) = claim(rig, key).await;
    send(&mut channel, "x").await;
    send(&mut channel, "n").await; // next style
    send(&mut channel, "y").await;
    (session, channel)
}

/// The raw bytes of the current screen after a full redraw (box-drawing characters kept).
async fn raw_full(channel: &mut Channel<client::Msg>) -> String {
    channel.window_change(80, 24, 0, 0).await.unwrap();
    let mut raw = String::new();
    while let Ok(msg) = timeout(Duration::from_millis(200), channel.wait()).await {
        match msg {
            Some(ChannelMsg::Data { data }) => raw.push_str(&String::from_utf8_lossy(&data)),
            Some(ChannelMsg::Close | ChannelMsg::Eof) | None => break,
            Some(_) => {}
        }
    }
    raw
}

const DOWN: &str = "\u{1b}[B";
const UP: &str = "\u{1b}[A";

#[tokio::test]
async fn the_layout_and_theme_switch_live_and_are_remembered() {
    let rig = Rig::start().await;
    let key = new_client_key();
    let (_session, mut channel) = at_main_menu(&rig, &key).await;

    // Classic by default: a box (the Unicode tier was chosen), no pointer, the plain footer.
    let classic = raw_full(&mut channel).await;
    assert!(classic.contains('\u{250c}'), "a box is drawn");
    assert!(!classic.contains("> ["), "no lightbar pointer");

    // Terminal settings shows the choices; `l` switches to the lightbar with no reconnect.
    send(&mut channel, "s").await;
    let settings = full(&mut channel).await;
    assert!(settings.contains("Layout: classic"), "{settings}");
    assert!(settings.contains("Theme: default"), "{settings}");
    let (switched, closed) = send(&mut channel, "l").await;
    assert!(!closed);
    assert!(switched.contains("Layout: lightbar"), "{switched}");
    assert!(switched.contains("Notice: Saved."), "{switched}");
    send(&mut channel, "\u{1b}").await;
    let lightbar = full(&mut channel).await;
    assert!(lightbar.contains("Main menu"), "{lightbar}");
    // The cursor stays on the item last used, so Esc from settings lands on it.
    assert!(lightbar.contains("> [s] Terminal settings"), "{lightbar}");
    assert!(lightbar.contains("Up/Down: move"), "{lightbar}");

    // Down moves the highlight, Up moves it back, Enter picks the highlighted row.
    send(&mut channel, DOWN).await;
    let moved = full(&mut channel).await;
    assert!(moved.contains("> [o] Sysop tools"), "{moved}");
    send(&mut channel, UP).await;
    assert!(full(&mut channel).await.contains("> [s] Terminal settings"));
    // Six rows up from "Terminal settings": Invites, Community (M3-J), Members, Doors (M4-D0),
    // Chat (M3-F), Files.
    for _ in 0..6 {
        send(&mut channel, UP).await;
    }
    assert!(full(&mut channel).await.contains("> [a] Files"));
    send(&mut channel, "\r").await;
    let picked = full(&mut channel).await;
    assert!(
        picked.contains("Notice: File areas are not built yet."),
        "{picked}"
    );
    // Hotkeys still work in the lightbar.
    send(&mut channel, "a").await;
    assert!(
        full(&mut channel)
            .await
            .contains("Notice: File areas are not built yet.")
    );

    // Compact: no box.
    send(&mut channel, "s").await;
    send(&mut channel, "l").await;
    let (compact_settings, _) = send(&mut channel, "\u{1b}").await;
    let compact = raw_full(&mut channel).await;
    assert!(
        compact.contains("Main menu"),
        "{compact_settings} {compact}"
    );
    assert!(!compact.contains('\u{250c}'), "no box in compact");
    assert!(!compact.contains("> ["), "no pointer in compact");

    // Back to classic, then step through the themes: amber, green, contrast, default.
    send(&mut channel, "s").await;
    send(&mut channel, "l").await;
    send(&mut channel, "t").await;
    let amber = full(&mut channel).await;
    assert!(amber.contains("Theme: amber"), "{amber}");
    send(&mut channel, "t").await;
    assert!(full(&mut channel).await.contains("Theme: green"));
    send(&mut channel, "\u{1b}").await;
    let green = full(&mut channel).await;
    assert!(
        green.contains("MAIN MENU"),
        "the theme's own wording: {green}"
    );
    assert!(!green.contains("SERVER-READABLE"), "{green}");
    assert!(
        !green.contains("The server can read what you do here."),
        "{green}"
    );
    assert!(green.contains("?: HELP"), "{green}");
    assert!(
        raw_full(&mut channel).await.contains('\u{250c}'),
        "classic box again"
    );

    // The choice is stored: a new connection with the same key starts in it.
    let _ = send(&mut channel, "l").await; // log off
    let session = login(rig.addr(), "anyone", &key).await;
    let mut again = common::open_shell(&session, 80, 24).await;
    words(&mut again).await;
    send(&mut again, "x").await;
    let remembered = full(&mut again).await;
    assert!(remembered.contains("MAIN MENU"), "{remembered}");
    rig.server.shutdown().await;
}

#[tokio::test]
async fn cursor_keys_do_nothing_in_the_classic_layout() {
    let rig = Rig::start().await;
    let key = new_client_key();
    let (_session, mut channel) = at_main_menu(&rig, &key).await;
    send(&mut channel, DOWN).await;
    let (after, closed) = send(&mut channel, "\r").await;
    assert!(!closed);
    let screen = format!("{after} {}", full(&mut channel).await);
    assert!(screen.contains("Main menu"), "{screen}");
    assert!(
        !screen.contains("not built yet.") && !screen.contains("Terminal settings Width"),
        "Enter acted on a selection the member cannot see: {screen}"
    );
    rig.server.shutdown().await;
}
