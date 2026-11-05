//! Interaction over a real russh client (UI-9): the mouse, the command palette and toasts, as
//! the founder (who holds every permission) uses them. Mouse reporting is turned on in the
//! settings screen, reports are sent as a terminal would send them, and the client's terminal is
//! seen to be put back when the session ends. Keys are generated here; the setup token comes
//! from the server's own console.
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

/// Everything the server sent since the last read, raw (escape sequences and all).
async fn raw(channel: &mut Channel<client::Msg>) -> String {
    let mut out = String::new();
    while let Ok(msg) = timeout(Duration::from_millis(250), channel.wait()).await {
        match msg {
            Some(ChannelMsg::Data { data }) => out.push_str(&String::from_utf8_lossy(&data)),
            Some(ChannelMsg::Close | ChannelMsg::Eof) | None => break,
            Some(_) => {}
        }
    }
    out
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

const ON: &str = "\x1b[?1000h\x1b[?1006h";
const OFF: &str = "\x1b[?1006l\x1b[?1000l";

#[tokio::test]
async fn the_mouse_is_the_members_choice_reports_are_bounded_and_the_terminal_is_put_back() {
    let rig = Rig::start().await;
    let key = new_client_key();
    let (_session, mut channel) = at_main_menu(&rig, &key).await;
    full(&mut channel).await;

    // Off until the member says so: nothing on the wire asked the client to report the mouse.
    channel.data_bytes(b"s".to_vec()).await.unwrap();
    let settings = raw(&mut channel).await;
    assert!(
        !settings.contains("1000h") && !settings.contains("1006h"),
        "{settings:?}"
    );
    assert!(full(&mut channel).await.contains("Mouse: off"));

    // `o` turns it on and saves; the very next frame tells the client to report the mouse.
    channel.data_bytes(b"o".to_vec()).await.unwrap();
    let on = raw(&mut channel).await;
    assert!(on.contains(ON), "{on:?}");
    assert!(full(&mut channel).await.contains("Mouse: on"));

    // Hostile and malformed reports: oversized, bad fields, a cut-off one. None is typed into
    // anything, none opens a menu item, and the screen is still the settings screen.
    let mut junk = Vec::new();
    junk.extend_from_slice(b"\x1b[<x;1;1M");
    junk.extend_from_slice(b"\x1b[<0;99999999;1M");
    junk.extend_from_slice(b"\x1b[<");
    junk.extend(std::iter::repeat_n(b'9', 300));
    junk.extend_from_slice(b"M");
    channel.data_bytes(junk).await.unwrap();
    raw(&mut channel).await;
    let still = full(&mut channel).await;
    assert!(
        still.contains("Mouse: on") && still.contains("Width: automatic"),
        "{still}"
    );

    // A wheel turn and a click far from anything do nothing at all.
    channel
        .data_bytes(b"\x1b[<65;3;3M\x1b[<0;200;60M".to_vec())
        .await
        .unwrap();
    raw(&mut channel).await;
    assert!(full(&mut channel).await.contains("Width: automatic"));

    // Back at the menu: Ctrl-K opens the palette (only on the menu), typing narrows it, Enter
    // runs the choice through the normal path, which opens the sysop menu.
    send(&mut channel, "\x1b").await;
    assert!(full(&mut channel).await.contains("Main menu"));
    channel.data_bytes(b"\x0b".to_vec()).await.unwrap();
    raw(&mut channel).await;
    let palette = full(&mut channel).await;
    assert!(
        palette.contains("Go to") && palette.contains("Type to search"),
        "{palette}"
    );
    send(&mut channel, "invite req").await;
    let narrowed = full(&mut channel).await;
    assert!(narrowed.contains("Invite requests"), "{narrowed}");
    channel.data_bytes(b"\r".to_vec()).await.unwrap();
    raw(&mut channel).await;
    let after = full(&mut channel).await;
    assert!(!after.contains("Go to"), "the palette closed: {after}");
    // The choice ran through the normal path: the sysop's queue screen is up.
    assert!(
        after.contains("Invite requests") || after.contains("No requests"),
        "{after}"
    );

    // Logging off puts the client's terminal back: mouse reporting off, before the screen
    // is left.
    channel.data_bytes(b"\x03".to_vec()).await.unwrap();
    let bye = raw(&mut channel).await;
    assert!(bye.contains(OFF), "{bye:?}");
    assert!(
        bye.find(OFF).unwrap() < bye.find("\x1b[?1049l").unwrap(),
        "{bye:?}"
    );
    rig.server.shutdown().await;
}

#[tokio::test]
async fn a_member_who_never_turned_the_mouse_on_can_do_everything_from_the_keyboard() {
    let rig = Rig::start().await;
    let key = new_client_key();
    let (_session, mut channel) = at_main_menu(&rig, &key).await;
    // Reports from a client that was never asked are dropped, not acted on.
    channel
        .data_bytes(b"\x1b[<0;5;5M\x1b[<64;5;5M".to_vec())
        .await
        .unwrap();
    raw(&mut channel).await;
    let menu = full(&mut channel).await;
    assert!(
        menu.contains("Main menu") && menu.contains("[s] Terminal settings"),
        "{menu}"
    );
    send(&mut channel, "s").await;
    assert!(full(&mut channel).await.contains("Mouse: off"));
    send(&mut channel, "\x1b").await;
    let (bye, closed) = send(&mut channel, "l").await;
    assert!(closed, "{bye}");
    rig.server.shutdown().await;
}
