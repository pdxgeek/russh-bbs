//! Internal mail over a real SSH client (M3-I, D78, D90): two members send and reply with a
//! quote, one deletes, one reports, `mail_per_day` refuses, and the database file holds no
//! plaintext. Temporary database, keys generated here (the mail key too).
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
use russh_bbs_core::mail::{KeyId, MailKey, MailKeys};
use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::store::{AccountState, AccountStore, NewAccount};
use russh_bbs_core::waiting_room::RatePolicy;
use russh_bbs_server::founder;
use russh_bbs_server::ssh::{HostKey, Limits, ServerConfig, SshServer};
use russh_bbs_server::ui::Ui;
use russh_bbs_store::SqliteStore;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};
use tempfile::TempDir;
use tokio::time::timeout;

struct Rig {
    server: SshServer,
    store: SqliteStore,
    db: SqlitePool,
    path: std::path::PathBuf,
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
        // The board has no sysop here: every account is made directly in the store.
        drop(console);
        let limits = Limits {
            ip_rate: RatePolicy::new(1_000, Duration::from_millis(1)),
            key_rate: RatePolicy::new(1_000, Duration::from_millis(1)),
            ..fast_limits()
        };
        let config = ServerConfig {
            listen: "127.0.0.1:0".parse().unwrap(),
            limits,
        };
        let keys = MailKeys::single(KeyId::FIRST, MailKey::generate().unwrap());
        let ui = Ui::standard().unwrap().with_mail_keys(Arc::new(keys));
        let server = SshServer::start_with_ui(
            &config,
            &host_key,
            Arc::new(store.clone()),
            Arc::new(gate),
            Arc::new(ui),
        )
        .await
        .unwrap();
        Self {
            server,
            store,
            db,
            path,
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

const SUBJECT: &str = "Quarterly kumquat inventory";
const TEXT: &str = "The kumquat count is eleven.";

/// Every byte of the database file and its write-ahead log, as one buffer.
fn raw_database(rig: &Rig) -> Vec<u8> {
    let mut bytes = std::fs::read(&rig.path).unwrap();
    let wal = rig.path.with_extension("db-wal");
    if let Ok(more) = std::fs::read(wal) {
        bytes.extend(more);
    }
    bytes
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle.as_bytes())
}

#[tokio::test]
async fn two_members_send_reply_with_a_quote_delete_and_report_and_the_file_holds_only_ciphertext()
{
    let rig = Rig::start().await;
    let ann_key = member(&rig, "ann").await;
    let bob_key = member(&rig, "bob").await;

    // D93: the welcome (login) screen carries no explanation of mail at rest.
    let session = login(rig.addr(), "anyone", &bob_key).await;
    let mut bob = common::open_shell(&session, 80, 24).await;
    let welcome = flat(&strip_ansi(&drain(&mut bob).await).replace(['|', '+'], " "));
    assert!(!welcome.contains("can read your mail"), "{welcome}");
    assert!(!welcome.contains("encrypted at rest"), "{welcome}");
    press(&mut bob, "x").await;
    press(&mut bob, "n").await;
    let menu = press(&mut bob, "y").await;
    assert!(flat(&menu).contains("Main menu"), "{menu}");
    // ... and neither does the help screen.
    let help = press(&mut bob, "?").await;
    assert!(!flat(&help).contains("can read your mail"), "{help}");
    press(&mut bob, "x").await;
    let (_ann_session, mut ann) = member_at_menu(&rig, &ann_key).await;

    // ann opens Mail: empty. The mail screens carry no notice about who can read it.
    let screen = press(&mut ann, "m").await;
    assert!(flat(&screen).contains("Your mailbox is empty."), "{screen}");
    let mut seen = flat(&screen);

    // ann writes to bob: handle, subject, then the editor.
    let screen = press(&mut ann, "c").await;
    assert!(
        flat(&screen).contains("Handle of the member to write to"),
        "{screen}"
    );
    seen.push_str(&flat(&screen));
    // A wrong handle is caught before anything is typed, with the one answer.
    line(&mut ann, "nobody").await;
    let screen = line(&mut ann, SUBJECT).await;
    assert!(
        flat(&screen).contains("No active member has that handle."),
        "{screen}"
    );
    let screen = line(&mut ann, "bob").await;
    seen.push_str(&flat(&screen));
    let editor = line(&mut ann, SUBJECT).await;
    assert!(
        flat(&editor).contains(&format!("To bob: {SUBJECT}")),
        "{editor}"
    );
    press(&mut ann, TEXT).await;
    let screen = press(&mut ann, SEND).await;
    assert!(flat(&screen).contains("Sent."), "{screen}");
    seen.push_str(&flat(&screen));

    // Nothing in the database file is readable text: not the subject, not the body.
    let file = raw_database(&rig);
    for word in ["kumquat", SUBJECT, TEXT, "eleven"] {
        assert!(!contains(&file, word), "{word} is in the database file");
    }
    assert_eq!(
        texts(&rig, "SELECT count(*) || '' FROM mail_message").await,
        ["1"]
    );
    assert!(
        texts(&rig, "SELECT count(*) || '' FROM draft").await == ["0"],
        "a sent message leaves no draft"
    );

    // bob sees it unread, opens it, replies with a quote.
    let screen = press(&mut bob, "m").await;
    let inbox = flat(&screen);
    assert!(
        inbox.contains("1 message, 1 unread") && inbox.contains("ann") && inbox.contains(SUBJECT),
        "{screen}"
    );
    seen.push_str(&inbox);
    let screen = press(&mut bob, "\r").await;
    let reading = flat(&screen);
    assert!(
        reading.contains("From ann") && reading.contains(TEXT),
        "{screen}"
    );
    let editor = press(&mut bob, "c").await;
    assert!(
        flat(&editor).contains(&format!("> {TEXT}")),
        "the quote starts the reply: {editor}"
    );
    assert!(
        flat(&editor).contains(&format!("Reply: Re: {SUBJECT}")),
        "{editor}"
    );
    press(&mut bob, "Ours is eleven too.").await;
    let screen = press(&mut bob, SEND).await;
    assert!(flat(&screen).contains("Sent."), "{screen}");

    // ann reads the reply, which carries the quote; the message is no longer unread.
    press(&mut ann, "\u{1b}").await; // the inbox -> the main menu
    let screen = press(&mut ann, "m").await; // ... and back in, to load the new mail
    let inbox = flat(&screen);
    assert!(
        inbox.contains("bob") && inbox.contains(&format!("Re: {SUBJECT}")),
        "{screen}"
    );
    let screen = press(&mut ann, "\r").await;
    let reading = flat(&screen);
    assert!(
        // The quote is drawn as a quote block (a bar, not the typed mark) since UI-4.
        reading.contains("Ours is eleven too.") && reading.contains(TEXT),
        "{screen}"
    );
    seen.push_str(&reading);

    // ann deletes the reply (asks first), and the mailbox is empty.
    let screen = press(&mut ann, "d").await;
    assert!(flat(&screen).contains("Delete this message?"), "{screen}");
    let screen = press(&mut ann, "y").await;
    assert!(
        flat(&screen).contains("Deleted.") && flat(&screen).contains("Your mailbox is empty."),
        "{screen}"
    );
    assert_eq!(
        texts(&rig, "SELECT count(*) || '' FROM mail_message").await,
        ["1"],
        "only bob's copy is left"
    );

    // No mail screen made the claims D78 forbids; the one explanation is on login and help only.
    for forbidden in ["private", "secure", "encrypt", "end-to-end", "sysop"] {
        assert!(
            !seen.to_lowercase().contains(forbidden),
            "{forbidden}: {seen}"
        );
    }

    // bob reports it: told what is copied, then the staff have a copy, audited by ids.
    let screen = press(&mut bob, "\r").await; // after sending, bob is at his inbox
    assert!(flat(&screen).contains(TEXT), "{screen}");
    let screen = press(&mut bob, "x").await;
    assert!(
        flat(&screen).contains("Send the staff a copy of this message"),
        "{screen}"
    );
    let screen = press(&mut bob, "y").await;
    assert!(
        flat(&screen).contains("The staff have a copy of that message."),
        "{screen}"
    );
    let copy = texts(
        &rig,
        "SELECT copy_subject || '|' || copy_body FROM mail_report",
    )
    .await;
    assert_eq!(
        copy,
        [format!("{SUBJECT}|{TEXT}")],
        "the report is the copy bob agreed to send"
    );
    let page = rig
        .store
        .page(&AuditQuery {
            action: Some(Action::MailReported),
            ..AuditQuery::default()
        })
        .await
        .unwrap();
    let events = known_events(page);
    assert_eq!(events.len(), 1);
    assert!(
        !events[0].event.detail.as_str().contains("kumquat"),
        "ids only"
    );
    let all = texts(&rig, "SELECT action || ' ' || detail FROM audit_log")
        .await
        .join("\n");
    assert!(!all.contains("kumquat") && !all.contains(SUBJECT));
    rig.stop().await;
}

#[tokio::test]
async fn mail_per_day_refuses_in_words_and_changes_nothing() {
    let rig = Rig::start().await;
    let ann_key = member(&rig, "ann").await;
    member(&rig, "bob").await;
    sqlx::query(
        "INSERT INTO member_limit (account_id, kind, value) \
         SELECT id, 'mail_per_day', 1 FROM accounts WHERE username = 'ann'",
    )
    .execute(&rig.db)
    .await
    .unwrap();
    let (_session, mut ann) = member_at_menu(&rig, &ann_key).await;
    press(&mut ann, "m").await;
    for (subject, body) in [("One", "first"), ("Two", "second")] {
        press(&mut ann, "c").await;
        line(&mut ann, "bob").await;
        line(&mut ann, subject).await;
        press(&mut ann, body).await;
        let screen = press(&mut ann, SEND).await;
        if subject == "One" {
            assert!(flat(&screen).contains("Sent."), "{screen}");
        } else {
            let said = flat(&screen);
            assert!(
                said.contains("You have reached your limit of 1 messages (mail_per_day)")
                    && said.contains("resets at 00:00 UTC")
                    && said.contains("kept until you leave mail"),
                "{screen}"
            );
        }
    }
    assert_eq!(
        texts(&rig, "SELECT count(*) || '' FROM mail_message").await,
        ["1"]
    );
    // The refused text is kept in memory for the session, never stored: no draft row exists.
    assert_eq!(texts(&rig, "SELECT count(*) || '' FROM draft").await, ["0"]);
    rig.stop().await;
}

#[tokio::test]
async fn a_board_built_without_the_mail_key_says_mail_is_not_available() {
    // The shared rig always has keys; build one by hand without them.
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(&dir.path().join("bbs.db")).await.unwrap();
    let host_key = HostKey::load_or_create(&dir.path().join("host_key")).unwrap();
    let account = store
        .create_account(NewAccount {
            username: sanitize_username("ann").unwrap(),
            role: Slug::MEMBER,
            state: AccountState::Active,
        })
        .await
        .unwrap();
    let key = new_client_key();
    store
        .attach_fingerprint(account.id, &fingerprint_of(&key))
        .await
        .unwrap();
    common::remember(fingerprint_of(&key), account.username.as_str());
    let config = ServerConfig {
        listen: "127.0.0.1:0".parse().unwrap(),
        limits: Limits {
            ip_rate: RatePolicy::new(1_000, Duration::from_millis(1)),
            key_rate: RatePolicy::new(1_000, Duration::from_millis(1)),
            ..fast_limits()
        },
    };
    let server = SshServer::start(&config, &host_key, Arc::new(store.clone()))
        .await
        .unwrap();
    let session = login(server.local_addr(), "anyone", &key).await;
    let mut channel = common::open_shell(&session, 80, 24).await;
    drain(&mut channel).await;
    press(&mut channel, "x").await;
    press(&mut channel, "n").await;
    press(&mut channel, "y").await;
    let screen = press(&mut channel, "m").await;
    assert!(
        flat(&screen).contains("Mail is not available on this board right now."),
        "{screen}"
    );
    server.shutdown().await;
    store.close().await;
}
