//! Feeds over a real SSH client (M3-G, D89): the sysop adds a feed from the forums screen, a
//! member reads it as a read-only board and cannot write to it, and the sysop removes it again.
//! Temporary database, keys generated here.
#![allow(
    clippy::assert_is_empty,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unused_async_trait_impl,
    clippy::too_many_lines,
    missing_docs
)]

mod common;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use common::{Client, fast_limits, new_client_key};
use russh::Channel;
use russh::ChannelMsg;
use russh::client::{AuthResult, Handle, Msg};
use russh::keys::{HashAlg, PrivateKey, PrivateKeyWithHashAlg};
use russh_bbs_core::access::{Fingerprint, Slug};
use russh_bbs_core::audit::{Action, AuditQuery, AuditStore, known_events};
use russh_bbs_core::feeds::{self, FeedStore};
use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::store::{AccountState, AccountStore, NewAccount, UnixSeconds};
use russh_bbs_core::waiting_room::RatePolicy;
use russh_bbs_server::founder;
use russh_bbs_server::ssh::{HostKey, Limits, ServerConfig, SshServer};
use russh_bbs_store::SqliteStore;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};
use tempfile::TempDir;
use tokio::time::timeout;

struct Rig {
    server: SshServer,
    store: SqliteStore,
    db: SqlitePool,
    token: String,
    _dir: TempDir,
}

impl Rig {
    async fn start() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bbs.db");
        let store = SqliteStore::open(&path).await.unwrap();
        let db = SqlitePool::connect_with(SqliteConnectOptions::new().filename(&path))
            .await
            .unwrap();
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
            ip_rate: RatePolicy::new(1_000, Duration::from_millis(1)),
            key_rate: RatePolicy::new(1_000, Duration::from_millis(1)),
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
            db,
            token,
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

/// The screen as a person sees it after sending `input`: drain what the server drew, force a
/// full redraw, and replay it onto a blank 80x24 screen.
async fn press(channel: &mut Channel<Msg>, input: &str) -> String {
    channel.data_bytes(input.as_bytes().to_vec()).await.unwrap();
    drain(channel).await;
    channel.window_change(80, 24, 0, 0).await.unwrap();
    strip_ansi(&drain(channel).await)
}

/// Type `text` and press Enter.
async fn line(channel: &mut Channel<Msg>, text: &str) -> String {
    press(channel, &format!("{text}\r")).await
}

/// Claim the board and get to the main menu (the icon check answered).
async fn founder_at_menu(rig: &Rig) -> (Handle<Client>, Channel<Msg>, PrivateKey) {
    let key = new_client_key();
    let session = login(rig.addr(), "claim", &key).await;
    let mut channel = common::open_shell(&session, 80, 24).await;
    drain(&mut channel).await;
    line(&mut channel, &rig.token).await;
    press(&mut channel, "x").await;
    press(&mut channel, "n").await;
    let menu = press(&mut channel, "y").await;
    assert!(flat(&menu).contains("Main menu"), "{menu}");
    (session, channel, key)
}

fn fingerprint_of(key: &PrivateKey) -> Fingerprint {
    Fingerprint::parse(&key.public_key().fingerprint(HashAlg::Sha256).to_string()).unwrap()
}

async fn texts(rig: &Rig, sql: &str) -> Vec<String> {
    sqlx::query_scalar(sql).fetch_all(&rig.db).await.unwrap()
}

/// Make a member with a fresh key.
async fn member(rig: &Rig, name: &str) -> PrivateKey {
    let key = new_client_key();
    let account = rig
        .store
        .create_account(NewAccount {
            username: sanitize_username(name).unwrap(),
            role: Slug::MEMBER,
            state: AccountState::Active,
        })
        .await
        .unwrap();
    rig.store
        .attach_fingerprint(account.id, &fingerprint_of(&key))
        .await
        .unwrap();
    key
}

/// Sign in as a member and get past the login screen and the first-run icon question.
async fn member_at_menu(rig: &Rig, key: &PrivateKey) -> (Handle<Client>, Channel<Msg>) {
    let session = login(rig.addr(), "anyone", key).await;
    let mut channel = common::open_shell(&session, 80, 24).await;
    drain(&mut channel).await;
    press(&mut channel, "x").await;
    press(&mut channel, "n").await;
    let menu = press(&mut channel, "y").await;
    assert!(flat(&menu).contains("Main menu"), "{menu}");
    (session, channel)
}

/// Ctrl-X: send, in the editor.

#[tokio::test]
async fn the_sysop_adds_a_feed_a_member_reads_it_read_only_and_the_sysop_removes_it() {
    let rig = Rig::start().await;
    let (_sysop_session, mut sysop, _) = founder_at_menu(&rig).await;
    let ann_key = member(&rig, "ann").await;
    let (_ann_session, mut ann) = member_at_menu(&rig, &ann_key).await;

    // A member is offered no feed keys.
    let screen = press(&mut ann, "f").await;
    assert!(!flat(&screen).contains("add a feed"), "{screen}");
    press(&mut ann, "f").await; // pressing it does nothing
    press(&mut ann, "\u{1b}").await;

    // The sysop adds one: an address, a name, and whether plain http is allowed.
    let screen = press(&mut sysop, "f").await;
    assert!(flat(&screen).contains("f add a feed"), "{screen}");
    press(&mut sysop, "f").await;
    let screen = line(&mut sysop, "ftp://example.org/rss").await;
    assert!(
        flat(&screen).contains("That address cannot be used."),
        "{screen}"
    );
    // Back to a clean prompt.
    for _ in 0.."ftp://example.org/rss".len() {
        press(&mut sysop, "\u{7f}").await;
    }
    let screen = line(&mut sysop, "https://example.org/rss?token=SECRET").await;
    assert!(flat(&screen).contains("Name of the board"), "{screen}");
    let screen = line(&mut sysop, "News").await;
    assert!(flat(&screen).contains("Allow plain http"), "{screen}");
    let screen = press(&mut sysop, "n").await;
    let seen = flat(&screen);
    assert!(
        seen.contains("Feed added.") && seen.contains("1. News [feed] (0 threads)"),
        "{screen}"
    );
    assert!(!seen.contains("private"), "{screen}");

    // The scheduled job would read it; here its items are stored through the same path.
    let doc = "<?xml version=\"1.0\"?><rss version=\"2.0\"><channel><title>t</title>\
        <item><title>Quokka sighted</title><guid>g1</guid><link>https://example.org/q</link>\
        <description><![CDATA[<p>A <b>quokka</b> was seen.</p>]]></description></item>\
        </channel></rss>";
    let parsed = feeds::parse(doc.as_bytes(), UnixSeconds::from_raw(1_800_000_000)).unwrap();
    let feed = rig.store.feeds().await.unwrap().remove(0);
    rig.store
        .ingest(
            feed.id,
            &parsed.items,
            UnixSeconds::from_raw(1_800_000_100),
            200,
        )
        .await
        .unwrap()
        .unwrap();

    // ann sees the board marked as a feed, reads it, and cannot write to it.
    let screen = press(&mut ann, "f").await;
    assert!(
        flat(&screen).contains("1. News [feed] (1 thread)")
            && !flat(&screen).contains("n new board"),
        "{screen}"
    );
    let screen = press(&mut ann, "1").await;
    let seen = flat(&screen);
    assert!(
        seen.contains("Quokka sighted") && seen.contains("[feed,"),
        "the author column reads feed: {screen}"
    );
    assert!(
        seen.contains("a feed: read only") && !seen.contains("w write"),
        "{screen}"
    );
    let after_w = press(&mut ann, "w").await;
    assert!(
        flat(&after_w).contains("Quokka sighted") && !flat(&after_w).contains("New thread"),
        "w opens nothing on a feed board: {after_w}"
    );
    let screen = press(&mut ann, "\r").await;
    let seen = flat(&screen);
    assert!(
        seen.contains("A quokka was seen.") && seen.contains("Link: https://example.org/q"),
        "{screen}"
    );
    assert!(!seen.contains("<b>") && !seen.contains("<p>"), "{screen}");
    // The new paging keys work on a feed item as on any post, and change nothing about writing.
    let paged = press(&mut ann, "\u{1b}[6~").await;
    assert!(flat(&paged).contains("A quokka was seen."), "{paged}");
    let paged = press(&mut ann, "\u{1b}[5~").await;
    assert!(flat(&paged).contains("A quokka was seen."), "{paged}");
    let after_r = press(&mut ann, "r").await;
    assert!(
        flat(&after_r).contains("A quokka was seen.") && !flat(&after_r).contains("Reply:"),
        "r opens no editor on a feed board: {after_r}"
    );
    assert_eq!(texts(&rig, "SELECT body FROM post").await.len(), 1);
    press(&mut ann, "\u{1b}").await;
    press(&mut ann, "\u{1b}").await;

    // The sysop publishes the board (and then takes it back), then removes the feed.
    press(&mut sysop, "1").await;
    let screen = press(&mut sysop, "p").await;
    assert!(flat(&screen).contains("can now be read as RSS"), "{screen}");
    assert_eq!(
        texts(&rig, "SELECT CAST(board_id AS TEXT) FROM board_public")
            .await
            .len(),
        1
    );
    let screen = press(&mut sysop, "p").await;
    assert!(
        flat(&screen).contains("no longer served as RSS"),
        "{screen}"
    );
    assert!(
        texts(&rig, "SELECT CAST(board_id AS TEXT) FROM board_public")
            .await
            .is_empty()
    );
    let screen = press(&mut sysop, "x").await;
    assert!(
        flat(&screen).contains("Remove this feed and its board"),
        "{screen}"
    );
    let screen = press(&mut sysop, "n").await; // keep it
    assert!(flat(&screen).contains("Quokka sighted"), "{screen}");
    press(&mut sysop, "x").await;
    let screen = press(&mut sysop, "y").await;
    let seen = flat(&screen);
    assert!(
        seen.contains("Feed removed.") && seen.contains("There are no boards you can open yet."),
        "{screen}"
    );
    assert!(texts(&rig, "SELECT subject FROM post").await.is_empty());

    // The log says who did what, and never the address.
    let log = known_events(rig.store.page(&AuditQuery::default()).await.unwrap());
    let actions: Vec<_> = log.iter().map(|e| e.event.action).collect();
    for expected in [
        Action::FeedAdded,
        Action::BoardPublished,
        Action::BoardUnpublished,
        Action::FeedRemoved,
    ] {
        assert!(actions.contains(&expected), "{expected:?} in {actions:?}");
    }
    for entry in &log {
        let detail = entry.event.detail.as_str();
        assert!(
            !detail.contains("example.org") && !detail.contains("SECRET"),
            "{detail}"
        );
    }
    rig.stop().await;
}
