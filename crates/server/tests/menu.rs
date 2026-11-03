//! The main menu over a real russh client (M2-A): the founder claims, answers the icon check,
//! sees the main menu, opens Terminal settings, goes back and logs off; the universal keys work;
//! the security marker is on the reserved last row; and an unknown key still sees only the
//! waiting room. Keys are generated here; the setup token comes from the server's own console.
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

/// Claim the board as the founder with `key`; returns the open channel and the login screen.
async fn claim(rig: &Rig, key: &PrivateKey) -> (Handle<Client>, Channel<client::Msg>, String) {
    let session = login(rig.addr(), "claim", key).await;
    let mut channel = common::open_shell(&session, 80, 24).await;
    words(&mut channel).await;
    let (screen, _) = send(&mut channel, &format!("{}\r", rig.token)).await;
    (session, channel, screen)
}

/// The whole current screen. ratatui sends only changed cells, so an incremental read can miss
/// letters that did not change; a window-change makes the server clear and redraw everything.
async fn full(channel: &mut Channel<client::Msg>) -> String {
    channel.window_change(80, 24, 0, 0).await.unwrap();
    words(channel).await.0
}

const QUESTION: &str = "Do these look like icons?";

/// Claim, answer the icon check with "yes" and return the channel at the main menu.
async fn at_main_menu(rig: &Rig, key: &PrivateKey) -> (Handle<Client>, Channel<client::Msg>) {
    let (session, mut channel, _) = claim(rig, key).await;
    send(&mut channel, "x").await;
    send(&mut channel, "n").await; // next style
    send(&mut channel, "y").await;
    (session, channel)
}

#[tokio::test]
async fn the_founder_walks_main_menu_settings_back_and_log_off() {
    let rig = Rig::start().await;
    let key = new_client_key();
    let (_session, mut channel) = at_main_menu(&rig, &key).await;

    let menu = full(&mut channel).await;
    for expected in [
        "Main menu",
        "[s] Terminal settings",
        "[l] Log off",
        "[o] Sysop tools",
        "[m] Mail (not built yet)",
        "?: help Esc: back Ctrl C: log off",
    ] {
        assert!(menu.contains(expected), "{expected:?} missing: {menu}");
    }
    // The server-readable marker is drawn in the Unicode tier (the style is checked by the
    // unit tests on the cell buffer; here the codepoints must arrive).
    assert!(
        menu.contains("The server can read what you do here."),
        "{menu}"
    );

    // Terminal settings is a menu item; Esc and the Back item both return to the same menu.
    send(&mut channel, "s").await;
    let settings = full(&mut channel).await;
    assert!(settings.contains("Width: automatic"), "{settings}");
    send(&mut channel, "\u{1b}").await;
    let again = full(&mut channel).await;
    assert!(again.contains("Main menu"), "{again}");
    send(&mut channel, "s").await;
    send(&mut channel, "b").await;
    assert!(full(&mut channel).await.contains("Main menu"));

    // A placeholder says so and nothing else happens.
    send(&mut channel, "m").await;
    let note = full(&mut channel).await;
    assert!(note.contains("Notice: Mail is not built yet."), "{note}");

    let (bye, closed) = send(&mut channel, "l").await;
    assert!(bye.contains("Goodbye, founder."), "{bye}");
    assert!(closed, "the session ends after log off");
    rig.server.shutdown().await;
}

#[tokio::test]
async fn help_back_and_quit_are_the_same_on_the_submenu() {
    let rig = Rig::start().await;
    let key = new_client_key();
    let (_session, mut channel) = at_main_menu(&rig, &key).await;

    send(&mut channel, "?").await;
    let help = full(&mut channel).await;
    for expected in ["Help", "Esc", "Ctrl C", "Any key: close help"] {
        assert!(help.contains(expected), "{expected:?} missing: {help}");
    }
    send(&mut channel, "x").await;
    assert!(full(&mut channel).await.contains("Main menu"));

    // The founder is a sysop, so the sysop submenu opens; help and back work there too.
    send(&mut channel, "o").await;
    let sub = full(&mut channel).await;
    assert!(sub.contains("Sysop tools"), "{sub}");
    assert!(sub.contains("[q] Invite requests"), "{sub}");
    assert!(!sub.contains("not built yet"), "{sub}");
    send(&mut channel, "?").await;
    assert!(full(&mut channel).await.contains("Any key: close help"));
    send(&mut channel, "x").await;
    send(&mut channel, "\u{1b}").await;
    assert!(full(&mut channel).await.contains("Main menu"));
    // Esc at the top of the tree does nothing.
    send(&mut channel, "\u{1b}").await;
    assert!(full(&mut channel).await.contains("Main menu"));

    // Ctrl-C logs off from a submenu.
    send(&mut channel, "o").await;
    let (bye, closed) = send(&mut channel, "\u{3}").await;
    assert!(bye.contains("Goodbye"), "{bye}");
    assert!(closed);
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
    let (after, _) = send(&mut channel, "?sofmuolq").await;
    let after = format!("{screen} {after} {}", full(&mut channel).await);
    for leaked in [
        "Main menu",
        "Sysop tools",
        "Terminal settings",
        "Forums",
        "Approval queue",
        "The server can read what you do here",
        QUESTION,
    ] {
        assert!(!after.contains(leaked), "{leaked:?} leaked: {after}");
    }
    rig.server.shutdown().await;
}
