//! The forums over a real SSH client (M3-E, D83): three members create a board, post, reply
//! with a quote, search, and a member without access cannot find or open a restricted board.
//! Temporary database, keys generated here.
#![allow(
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
use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::store::{AccountState, AccountStore, NewAccount};
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
            &common::user_for(key, user),
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
    line(&mut channel, "founder password one").await;
    line(&mut channel, "founder password one").await;
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
    common::remember(fingerprint_of(&key), account.username.as_str());
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
const SEND: &str = "\u{18}";

#[tokio::test]
async fn three_members_post_reply_search_and_a_restricted_board_stays_hidden() {
    let rig = Rig::start().await;
    let (_sysop_session, mut sysop, _) = founder_at_menu(&rig).await;
    let ann_key = member(&rig, "ann").await;
    let bob_key = member(&rig, "bob").await;
    let (_ann_session, mut ann) = member_at_menu(&rig, &ann_key).await;
    let (_bob_session, mut bob) = member_at_menu(&rig, &bob_key).await;

    // The sysop opens the forums: no boards yet, and "n" makes one.
    let screen = press(&mut sysop, "f").await;
    assert!(
        flat(&screen).contains("There are no boards you can open yet."),
        "{screen}"
    );
    assert!(flat(&screen).contains("n new board"), "{screen}");
    press(&mut sysop, "n").await;
    line(&mut sysop, "General").await;
    line(&mut sysop, "Talk about anything").await;
    let screen = press(&mut sysop, "n").await; // open to every member
    assert!(
        flat(&screen).contains("Board created.") && flat(&screen).contains("1. General"),
        "{screen}"
    );
    press(&mut sysop, "n").await;
    line(&mut sysop, "Staff room").await;
    line(&mut sysop, "Rotas").await;
    let screen = press(&mut sysop, "y").await; // restricted
    assert!(flat(&screen).contains("2. Staff room"), "{screen}");

    // The sysop starts a thread in General through the editor.
    press(&mut sysop, "1").await;
    press(&mut sysop, "w").await;
    let editor = line(&mut sysop, "Welcome").await;
    assert!(flat(&editor).contains("General: Welcome"), "{editor}");
    press(
        &mut sysop,
        "Hello everyone, who grows tomatoes?\rSecond line.",
    )
    .await;
    let screen = press(&mut sysop, SEND).await;
    let reading = flat(&screen);
    assert!(
        reading.contains("Note: Posted.")
            && reading.contains("Post 1 of 1")
            && reading.contains("General"),
        "{screen}"
    );
    assert!(
        reading.contains("Hello everyone, who grows tomatoes?"),
        "{screen}"
    );
    assert!(
        !reading.contains("private"),
        "nothing is called private: {screen}"
    );

    // ... and one in the restricted board.
    press(&mut sysop, "\u{1b}").await; // reader -> thread list
    press(&mut sysop, "\u{1b}").await; // -> boards
    press(&mut sysop, "2").await;
    line_after(&mut sysop, "w", "Rota").await;
    press(&mut sysop, "Keys are in the zebrafish drawer.").await;
    press(&mut sysop, SEND).await;

    // ann sees General only, reads the welcome thread and replies with a quote.
    let screen = press(&mut ann, "f").await;
    assert!(
        flat(&screen).contains("1. General") && !flat(&screen).contains("Staff room"),
        "{screen}"
    );
    assert!(!flat(&screen).contains("n new board"), "{screen}");
    let screen = press(&mut ann, "1").await;
    assert!(flat(&screen).contains("Welcome"), "{screen}");
    let screen = press(&mut ann, "\r").await; // open the thread
    assert!(
        flat(&screen).contains("From ") && flat(&screen).contains("tomatoes"),
        "{screen}"
    );
    let editor = press(&mut ann, "c").await;
    assert!(
        flat(&editor).contains("> Hello everyone"),
        "the quote starts the reply: {editor}"
    );
    press(&mut ann, "Me! Two plants.").await;
    let screen = press(&mut ann, SEND).await;
    let reading = flat(&screen);
    assert!(
        reading.contains("2 of 2") && reading.contains("Me! Two plants."),
        "{screen}"
    );
    assert!(
        // The quote is drawn as a quote block (a bar, not the typed mark) since UI-4.
        reading.contains("Hello everyone"),
        "the quote was kept: {screen}"
    );

    // bob searches: General's text is found; the staff room's is not, and not even a count.
    press(&mut bob, "f").await;
    press(&mut bob, "s").await;
    let screen = line(&mut bob, "tomatoes").await;
    assert!(flat(&screen).contains("General Welcome"), "{screen}");
    press(&mut bob, "s").await;
    let hidden = line(&mut bob, "zebrafish").await;
    let nothing = {
        press(&mut bob, "s").await;
        line(&mut bob, "qqqqzzzz").await
    };
    assert!(flat(&hidden).contains("Nothing found."), "{hidden}");
    assert_eq!(
        flat(&hidden).replace("zebrafish", "qqqqzzzz"),
        flat(&nothing),
        "the same answer"
    );
    assert!(
        !flat(&hidden).contains("Rota") && !flat(&hidden).contains("Staff"),
        "{hidden}"
    );
    // Hostile search text is just words.
    press(&mut bob, "s").await;
    let screen = line(&mut bob, "\"tomatoes\" NEAR( * AND \"").await;
    assert!(!flat(&screen).contains("did not work"), "{screen}");

    // The sysop lets ann in; ann now finds and opens it, bob still cannot.
    press(&mut sysop, "\u{1b}").await; // reader -> thread list
    let screen = press(&mut sysop, "a").await;
    assert!(
        flat(&screen).contains("Handle of the member to let into this board"),
        "{screen}"
    );
    let screen = line(&mut sysop, "ann").await;
    assert!(
        flat(&screen).contains("They can now use this board."),
        "{screen}"
    );
    press(&mut ann, "\u{1b}").await;
    press(&mut ann, "\u{1b}").await;
    press(&mut ann, "\u{1b}").await;
    let screen = press(&mut ann, "f").await;
    assert!(flat(&screen).contains("2. Staff room"), "{screen}");
    press(&mut ann, "s").await;
    let found = line(&mut ann, "zebrafish").await;
    assert!(flat(&found).contains("Staff room Rota"), "{found}");
    let bob_list = {
        press(&mut bob, "\u{1b}").await;
        press(&mut bob, "\u{1b}").await;
        press(&mut bob, "f").await
    };
    assert!(!flat(&bob_list).contains("Staff room"), "{bob_list}");

    // What was stored: posts exist, the audit has the board and grant rows, and no row holds
    // an escape character.
    assert_eq!(texts(&rig, "SELECT count(*) || '' FROM post").await, ["3"]);
    let page = rig
        .store
        .page(&AuditQuery {
            action: Some(Action::BoardCreated),
            ..AuditQuery::default()
        })
        .await
        .unwrap();
    assert_eq!(known_events(page).len(), 2);
    let page = rig
        .store
        .page(&AuditQuery {
            action: Some(Action::PermissionGranted),
            ..AuditQuery::default()
        })
        .await
        .unwrap();
    assert_eq!(known_events(page).len(), 4, "four allow entries for ann");
    rig.stop().await;
}

/// Press `key`, then type `text` and Enter (the next prompt).
async fn line_after(channel: &mut Channel<Msg>, key: &str, text: &str) -> String {
    press(channel, key).await;
    line(channel, text).await
}

/// Wait until `sql` returns exactly `want`, or fail with what it last returned.
async fn until_texts(rig: &Rig, sql: &str, want: &[String]) {
    let mut last = Vec::new();
    for _ in 0..200 {
        last = texts(rig, sql).await;
        if last == want {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("{sql}: wanted {want:?}, last saw {last:?}");
}

/// FIX-M3-7 (D133): the text in an open forum editor survives a lost connection. The first
/// change is autosaved while the member is still typing; more typing after that is stored when
/// the connection drops (no Ctrl-C, no Esc, no save key), and the next session resumes it.
#[tokio::test]
async fn a_forum_draft_survives_a_dropped_ssh_connection() {
    let rig = Rig::start().await;
    let (_sysop_session, mut sysop, _) = founder_at_menu(&rig).await;
    press(&mut sysop, "f").await;
    press(&mut sysop, "n").await;
    line(&mut sysop, "General").await;
    line(&mut sysop, "Talk about anything").await;
    press(&mut sysop, "n").await;
    let ann_key = member(&rig, "ann").await;

    let (session, mut ann) = member_at_menu(&rig, &ann_key).await;
    press(&mut ann, "f").await;
    press(&mut ann, "1").await;
    press(&mut ann, "w").await;
    let editor = line(&mut ann, "Tomatoes").await;
    assert!(flat(&editor).contains("General: Tomatoes"), "{editor}");

    // While typing: the draft is stored without any save key.
    press(&mut ann, "first part").await;
    until_texts(&rig, "SELECT body FROM draft", &["first part".to_owned()]).await;

    // More typing, then the connection is lost: no Ctrl-C, no Esc, nothing said.
    press(&mut ann, " and the last words").await;
    drop(ann);
    drop(session);
    until_texts(
        &rig,
        "SELECT body FROM draft",
        &["first part and the last words".to_owned()],
    )
    .await;

    // The next session opens the same compose and finds the text.
    let session2 = login(rig.addr(), "anyone", &ann_key).await;
    let mut ann = common::open_shell(&session2, 80, 24).await;
    drain(&mut ann).await;
    let menu = press(&mut ann, "x").await; // past the welcome; the icon questions are answered
    assert!(flat(&menu).contains("Main menu"), "{menu}");
    press(&mut ann, "f").await;
    press(&mut ann, "1").await;
    press(&mut ann, "w").await;
    let editor = line(&mut ann, "Tomatoes").await;
    assert!(
        flat(&editor).contains("first part and the last words"),
        "{editor}"
    );
    rig.stop().await;
}
