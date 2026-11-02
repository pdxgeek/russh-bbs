//! The sysop queue over a real SSH client (M2-E): a stranger asks for an invite and always reads
//! the same words, the sysop and a trusted member see the request on the 80x24 screens and
//! decide or vouch, every decision is audited without the request's text, and each permission
//! gates its path. Everything uses a temporary database and keys generated here.
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
use russh_bbs_core::audit::{Action, AuditQuery, AuditStore};
use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::store::{AccountState, AccountStore, NewAccount};
use russh_bbs_core::waiting_room::RatePolicy;
use russh_bbs_server::founder;
use russh_bbs_server::ssh::{HostKey, Limits, ServerConfig, SshServer};
use russh_bbs_store::SqliteStore;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};
use tempfile::TempDir;
use tokio::time::timeout;

/// What a stranger reads after submitting the request form, whatever became of the request.
const ANSWER: &str = "Thank you. If your request is kept";

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

    async fn sql(&self, sql: &str) {
        sqlx::query(sql).execute(&self.db).await.unwrap();
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

/// A stranger's key opens the waiting room: its home screen is returned.
async fn stranger(rig: &Rig) -> (Handle<Client>, Channel<Msg>, PrivateKey, String) {
    let key = new_client_key();
    let session = login(rig.addr(), "anyone", &key).await;
    let mut channel = common::open_shell(&session, 80, 24).await;
    drain(&mut channel).await;
    channel.window_change(80, 24, 0, 0).await.unwrap();
    let home = strip_ansi(&drain(&mut channel).await);
    (session, channel, key, home)
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

async fn count(rig: &Rig, table: &str) -> i64 {
    sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
        .fetch_one(&rig.db)
        .await
        .unwrap()
}

async fn texts(rig: &Rig, sql: &str) -> Vec<String> {
    sqlx::query_scalar(sql).fetch_all(&rig.db).await.unwrap()
}

/// On the request form: type a handle and a message and press Enter; returns the screen.
async fn ask(channel: &mut Channel<Msg>, handle: &str, message: &str) -> String {
    line(channel, handle).await;
    line(channel, message).await
}

/// A stranger who has opened the request form.
async fn at_request_form(rig: &Rig) -> (Handle<Client>, Channel<Msg>, PrivateKey) {
    let (session, mut channel, key, _) = stranger(rig).await;
    let form = press(&mut channel, "i").await;
    assert!(flat(&form).contains("Request an invite"), "{form}");
    (session, channel, key)
}

/// Make an account holding `role` with a fresh key, as the board would after a decision.
async fn member_with(rig: &Rig, name: &str, role: Slug, state: AccountState) -> PrivateKey {
    let key = new_client_key();
    let account = rig
        .store
        .create_account(NewAccount {
            username: sanitize_username(name).unwrap(),
            role,
            state,
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

/// The queue screen as the founder (a sysop): Sysop tools, then the queue.
async fn open_queue(channel: &mut Channel<Msg>) -> String {
    press(channel, "o").await;
    press(channel, "q").await
}

fn assert_fits_80x24(screen: &str) {
    assert!(
        screen.lines().count() <= 24,
        "{} rows\n{screen}",
        screen.lines().count()
    );
    assert!(screen.lines().all(|l| l.chars().count() <= 80), "{screen}");
    let status = screen.lines().last().unwrap();
    assert!(
        status.contains("The server can read what you do here"),
        "status row: {status:?}"
    );
}

async fn columns(rig: &Rig, table: &str) -> Vec<String> {
    texts(
        rig,
        &format!("SELECT name FROM pragma_table_info('{table}') ORDER BY cid"),
    )
    .await
}

async fn pages(rig: &Rig) -> Vec<String> {
    let (_s, mut channel, _key, home) = stranger(rig).await;
    let rules = press(&mut channel, "r").await;
    press(&mut channel, "\u{1b}").await;
    let form = press(&mut channel, "i").await;
    let redeem = {
        press(&mut channel, "\u{1b}").await;
        press(&mut channel, "c").await
    };
    vec![home, rules, form, redeem]
}

#[tokio::test]
async fn a_stranger_reads_the_same_words_whatever_the_queue_does_with_the_request() {
    let rig = Rig::start().await;

    // The baseline: the first request is kept.
    let (_s1, mut first, _key1) = at_request_form(&rig).await;
    let kept = ask(&mut first, "hopeful", "a friend of a friend").await;
    assert!(flat(&kept).contains(ANSWER), "{kept}");
    assert!(flat(&kept).contains("Nobody is notified"), "{kept}");
    assert_eq!(count(&rig, "invite_request").await, 1);
    // What was typed does not stay on screen, so the screens can be compared whole.
    assert!(
        !kept.contains("hopeful") && !kept.contains("friend of a friend"),
        "{kept}"
    );

    // The same key again (already pending), a third time, and a fourth: the per-key limit is a
    // burst of three. The words never change and nothing more is stored.
    let again = ask(&mut first, "other-name", "second try").await;
    let third = ask(&mut first, "third-name", "third try").await;
    let limited = ask(&mut first, "fourth-name", "fourth try").await;
    for (what, screen) in [
        ("repeat", &again),
        ("third", &third),
        ("rate limited", &limited),
    ] {
        assert_eq!(screen, &kept, "{what}");
    }
    assert_eq!(count(&rig, "invite_request").await, 1);
    let kept_row = texts(&rig, "SELECT handle || '|' || message FROM invite_request").await;
    assert_eq!(
        kept_row,
        ["hopeful|a friend of a friend"],
        "the first request is untouched"
    );

    // A full queue: the cap is a sysop setting (queue_max_pending).
    rig.sql("INSERT INTO settings (key, value) VALUES ('queue_max_pending', '1')")
        .await;
    let (_s2, mut full, _) = at_request_form(&rig).await;
    let dropped = ask(&mut full, "latecomer", "no room").await;
    assert_eq!(dropped, kept, "queue full");
    assert_eq!(count(&rig, "invite_request").await, 1);
    rig.sql("DELETE FROM settings WHERE key = 'queue_max_pending'")
        .await;

    // A key that was just denied.
    let (_s3, mut denied, denied_key) = at_request_form(&rig).await;
    let hash = russh_bbs_core::queue::cooldown_key(&fingerprint_of(&denied_key));
    rig.sql(&format!(
        "INSERT INTO request_denial (key_hash, expires_at) VALUES ('{hash}', 99999999999)"
    ))
    .await;
    let cooling = ask(&mut denied, "denied-one", "please").await;
    assert_eq!(cooling, kept, "cooling down");

    // A key that already has an account that is not active (it still reaches the room).
    let suspended_key =
        member_with(&rig, "suspended-one", Slug::MEMBER, AccountState::Suspended).await;
    let session = login(rig.addr(), "anyone", &suspended_key).await;
    let mut channel = common::open_shell(&session, 80, 24).await;
    drain(&mut channel).await;
    press(&mut channel, "i").await;
    let bound = ask(&mut channel, "suspended-again", "let me back").await;
    assert_eq!(bound, kept, "key with an account");

    // The waiting snapshot without `invite.request` (a sysop took it away): same words.
    rig.sql("DELETE FROM role_permission WHERE role = 'waiting' AND permission = 'invite.request'")
        .await;
    let (_s4, mut gone, _) = at_request_form(&rig).await;
    let refused = ask(&mut gone, "no-permission", "please").await;
    assert_eq!(refused, kept, "permission not held");
    rig.sql("INSERT INTO role_permission (role, permission) VALUES ('waiting', 'invite.request')")
        .await;

    // After all of that exactly the one request exists, and nothing else was written.
    assert_eq!(count(&rig, "invite_request").await, 1);
    assert_eq!(count(&rig, "request_denial").await, 1);
    let audit = rig.store.page(&AuditQuery::default()).await.unwrap();
    assert!(
        audit.iter().all(|row| !matches!(
            row.event.action,
            Action::MemberApproved | Action::MemberDenied | Action::MemberVouched
        )),
        "submitting is not audited"
    );
    rig.stop().await;
}

#[tokio::test]
async fn a_request_stores_only_the_documented_fields_and_no_address() {
    let rig = Rig::start().await;
    let (_s, mut channel, key) = at_request_form(&rig).await;
    ask(&mut channel, "hopeful", "  hello there  ").await;

    assert_eq!(
        columns(&rig, "invite_request").await,
        [
            "id",
            "fingerprint",
            "handle",
            "message",
            "created_at",
            "expires_at"
        ]
    );
    assert_eq!(
        columns(&rig, "invite_request_vouch").await,
        ["request_id", "account_id", "created_at"]
    );
    assert_eq!(
        columns(&rig, "request_denial").await,
        ["key_hash", "expires_at"]
    );

    let row = texts(
        &rig,
        "SELECT fingerprint || '|' || handle || '|' || message FROM invite_request",
    )
    .await;
    assert_eq!(
        row,
        [format!("{}|hopeful|hello there", fingerprint_of(&key))]
    );
    let stored = row.join("");
    assert!(
        !stored.contains("127.0.0.1") && !stored.contains("::1"),
        "{stored}"
    );
    rig.stop().await;
}

#[tokio::test]
async fn the_waiting_room_screens_do_not_change_with_the_queue() {
    let rig = Rig::start().await;
    let empty = pages(&rig).await;

    // A queue with requests, vouches, a denial, and then a full one.
    let voucher = member_with(&rig, "voucher", Slug::TRUSTED, AccountState::Active).await;
    drop(voucher);
    for n in 1..=5 {
        rig.sql(&format!(
            "INSERT INTO invite_request (fingerprint, handle, message, created_at, expires_at) \
             VALUES ('SHA256:{n:A>43}', 'person{n}', 'hello', 100, 99999999999)"
        ))
        .await;
    }
    rig.sql(
        "INSERT INTO invite_request_vouch (request_id, account_id, created_at) VALUES (1, 1, 100)",
    )
    .await;
    rig.sql(&format!(
        "INSERT INTO request_denial (key_hash, expires_at) VALUES ('{}', 99999999999)",
        "0".repeat(64)
    ))
    .await;
    let busy = pages(&rig).await;
    rig.sql("INSERT INTO settings (key, value) VALUES ('queue_max_pending', '5')")
        .await;
    let full = pages(&rig).await;

    assert_eq!(
        empty, busy,
        "pending requests change nothing a stranger sees"
    );
    assert_eq!(empty, full, "a full queue changes nothing a stranger sees");
    for page in &empty {
        let lower = page.to_lowercase();
        for word in ["queue", "pending", "person1", "voucher", "waiting for"] {
            assert!(!lower.contains(word), "{word:?} leaks:\n{page}");
        }
    }
    rig.stop().await;
}

#[tokio::test]
async fn the_sysop_approves_from_the_screen_and_the_requester_becomes_a_member() {
    let rig = Rig::start().await;
    let (_s, mut asking, key) = at_request_form(&rig).await;
    ask(&mut asking, "hopeful", "friend of alice").await;

    let (_founder_session, mut founder, _) = founder_at_menu(&rig).await;
    let list = open_queue(&mut founder).await;
    let words = flat(&list);
    assert!(words.contains("Invite requests"), "{list}");
    assert!(words.contains("[1] hopeful"), "{list}");
    assert!(
        words.contains("asked 0m ago") && words.contains("vouches: 0"),
        "{list}"
    );
    assert!(words.contains("friend of alice"), "{list}");
    assert_fits_80x24(&list);

    let detail = press(&mut founder, "1").await;
    let words = flat(&detail);
    assert!(words.contains("Request 1: hopeful"), "{detail}");
    assert!(words.contains("Vouched for by: nobody yet"), "{detail}");
    assert!(words.contains("[a] Approve [d] Deny [v] Vouch"), "{detail}");
    assert_fits_80x24(&detail);

    let confirm = press(&mut founder, "a").await;
    assert!(flat(&confirm).contains("Approve hopeful?"), "{confirm}");
    // Anything but y cancels.
    let back = press(&mut founder, "n").await;
    assert!(flat(&back).contains("[a] Approve"), "{back}");
    assert_eq!(count(&rig, "invite_request").await, 1);
    press(&mut founder, "a").await;
    let done = press(&mut founder, "y").await;
    let words = flat(&done);
    assert!(words.contains("Approved."), "{done}");
    assert!(words.contains("No requests are waiting."), "{done}");
    assert_fits_80x24(&done);

    // The account exists, is active, is bound to the key that asked, and names the approver.
    assert_eq!(count(&rig, "invite_request").await, 0);
    let account = rig
        .store
        .account_by_fingerprint(&fingerprint_of(&key))
        .await
        .unwrap()
        .expect("the requesting key has an account");
    assert_eq!(account.username.as_str(), "hopeful");
    assert_eq!(account.state, AccountState::Active);
    let inviter = texts(
        &rig,
        "SELECT CAST(invited_by AS TEXT) FROM accounts WHERE username = 'hopeful'",
    )
    .await;
    assert_eq!(inviter, ["1"], "the approving sysop is recorded");
    let audit = rig.store.page(&AuditQuery::default()).await.unwrap();
    let approved: Vec<_> = audit
        .iter()
        .filter(|row| row.event.action == Action::MemberApproved)
        .collect();
    assert_eq!(approved.len(), 1);
    assert!(!format!("{audit:?}").contains("friend of alice"));

    // The requester reconnects with the same key and is a member.
    let session = login(rig.addr(), "anyone", &key).await;
    let mut channel = common::open_shell(&session, 80, 24).await;
    drain(&mut channel).await;
    channel.window_change(80, 24, 0, 0).await.unwrap();
    let screen = strip_ansi(&drain(&mut channel).await);
    assert!(flat(&screen).contains("Signed in as hopeful"), "{screen}");
    rig.stop().await;
}

#[tokio::test]
async fn the_sysop_denies_and_the_key_is_turned_away_with_the_same_words() {
    let rig = Rig::start().await;
    let (_s, mut asking, key) = at_request_form(&rig).await;
    let kept = ask(&mut asking, "unwelcome", "let me in").await;

    let (_fs, mut founder, _) = founder_at_menu(&rig).await;
    open_queue(&mut founder).await;
    press(&mut founder, "1").await;
    let confirm = press(&mut founder, "d").await;
    let words = flat(&confirm);
    assert!(words.contains("Deny unwelcome?"), "{confirm}");
    assert!(words.contains("turned away for 24 hours"), "{confirm}");
    let done = press(&mut founder, "y").await;
    assert!(
        flat(&done).contains("Denied. The request was deleted."),
        "{done}"
    );

    // The request is gone; all that remains of it is a hash and an expiry.
    assert_eq!(count(&rig, "invite_request").await, 0);
    let kept_rows = texts(
        &rig,
        "SELECT key_hash || '|' || CAST(expires_at AS TEXT) FROM request_denial",
    )
    .await;
    assert_eq!(kept_rows.len(), 1);
    assert!(kept_rows[0].starts_with(&russh_bbs_core::queue::cooldown_key(&fingerprint_of(&key))));
    assert!(!kept_rows[0].contains("unwelcome") && !kept_rows[0].contains("let me in"));
    assert_eq!(
        rig.store
            .account_by_fingerprint(&fingerprint_of(&key))
            .await
            .unwrap(),
        None,
        "no account"
    );
    let audit = rig.store.page(&AuditQuery::default()).await.unwrap();
    let denied: Vec<_> = audit
        .iter()
        .filter(|row| row.event.action == Action::MemberDenied)
        .collect();
    assert_eq!(denied.len(), 1);
    let dump = format!("{audit:?}");
    assert!(!dump.contains("unwelcome") && !dump.contains("let me in"));
    assert!(!dump.contains(fingerprint_of(&key).as_str()));

    // The same key asks again: the same answer, nothing stored.
    let session = login(rig.addr(), "anyone", &key).await;
    let mut channel = common::open_shell(&session, 80, 24).await;
    drain(&mut channel).await;
    press(&mut channel, "i").await;
    let again = ask(&mut channel, "unwelcome", "let me in").await;
    assert_eq!(again, kept);
    assert_eq!(count(&rig, "invite_request").await, 0);
    rig.stop().await;
}

#[tokio::test]
async fn a_trusted_member_vouches_once_and_the_sysop_sees_the_name() {
    let rig = Rig::start().await;
    let (_s, mut asking, _) = at_request_form(&rig).await;
    ask(&mut asking, "hopeful", "friend of voucher").await;
    let trusted = member_with(&rig, "voucher", Slug::TRUSTED, AccountState::Active).await;
    let plain = member_with(&rig, "plain-member", Slug::MEMBER, AccountState::Active).await;

    // A plain member has no vouch item, and its hotkey does nothing.
    {
        let (_ps, mut channel) = member_at_menu(&rig, &plain).await;
        let members = flat(&press(&mut channel, "u").await);
        assert!(members.contains("Member list"), "{members}");
        assert!(!members.contains("Vouch for a request"), "{members}");
        let after = flat(&press(&mut channel, "v").await);
        assert!(
            after.contains("Member list") && !after.contains("People who asked"),
            "{after}"
        );
    }

    let (_ts, mut channel) = member_at_menu(&rig, &trusted).await;
    let members = flat(&press(&mut channel, "u").await);
    assert!(members.contains("[v] Vouch for a request"), "{members}");
    let list = press(&mut channel, "v").await;
    let words = flat(&list);
    assert!(words.contains("Vouch for a request"), "{list}");
    assert!(
        words.contains("[1] hopeful") && words.contains("friend of voucher"),
        "{list}"
    );
    assert_fits_80x24(&list);
    let detail = press(&mut channel, "1").await;
    let words = flat(&detail);
    assert!(words.contains("[v] Vouch"), "{detail}");
    assert!(
        !words.contains("[a] Approve") && !words.contains("[d] Deny"),
        "a voucher cannot decide\n{detail}"
    );
    // 'a' and 'd' do nothing here.
    let still = flat(&press(&mut channel, "a").await);
    assert!(!still.contains("Approve hopeful?"), "{still}");
    let confirm = press(&mut channel, "v").await;
    assert!(flat(&confirm).contains("Vouch for hopeful?"), "{confirm}");
    let done = press(&mut channel, "y").await;
    let words = flat(&done);
    assert!(words.contains("Vouched."), "{done}");
    assert!(words.contains("vouches: 1 (yours included)"), "{done}");
    assert_eq!(count(&rig, "invite_request_vouch").await, 1);

    // Not twice: the key is no longer offered, and pressing it does nothing.
    let again = press(&mut channel, "1").await;
    assert!(!flat(&again).contains("[v] Vouch"), "{again}");
    press(&mut channel, "v").await;
    assert_eq!(count(&rig, "invite_request_vouch").await, 1);

    // The sysop sees the vouch, by name.
    let (_fs, mut founder, _) = founder_at_menu(&rig).await;
    let list = flat(&open_queue(&mut founder).await);
    assert!(list.contains("vouches: 1"), "{list}");
    let detail = flat(&press(&mut founder, "1").await);
    assert!(detail.contains("Vouched for by: voucher"), "{detail}");
    let audit = rig.store.page(&AuditQuery::default()).await.unwrap();
    assert_eq!(
        audit
            .iter()
            .filter(|row| row.event.action == Action::MemberVouched)
            .count(),
        1
    );
    rig.stop().await;
}

#[tokio::test]
async fn each_permission_gates_its_path_on_the_very_next_action() {
    let rig = Rig::start().await;
    let (_s, mut asking, _) = at_request_form(&rig).await;
    ask(&mut asking, "hopeful", "hello").await;
    let (_fs, mut founder, _) = founder_at_menu(&rig).await;
    open_queue(&mut founder).await;
    let detail = flat(&press(&mut founder, "1").await);
    assert!(
        detail.contains("[a] Approve [d] Deny [v] Vouch"),
        "{detail}"
    );

    // members.deny taken away: the key is not offered and nothing is deleted.
    rig.sql("DELETE FROM role_permission WHERE role = 'sysop' AND permission = 'members.deny'")
        .await;
    let without = flat(&press(&mut founder, "x").await);
    assert!(!without.contains("[d] Deny"), "{without}");
    press(&mut founder, "d").await;
    let still = flat(&press(&mut founder, "y").await);
    assert!(!still.contains("Denied"), "{still}");
    assert_eq!(count(&rig, "invite_request").await, 1);
    assert_eq!(count(&rig, "request_denial").await, 0);
    rig.sql("INSERT INTO role_permission (role, permission) VALUES ('sysop', 'members.deny')")
        .await;
    assert!(flat(&press(&mut founder, "x").await).contains("[d] Deny"));

    // members.vouch taken away: no vouch.
    rig.sql("DELETE FROM role_permission WHERE role = 'sysop' AND permission = 'members.vouch'")
        .await;
    let without = flat(&press(&mut founder, "x").await);
    assert!(!without.contains("[v] Vouch"), "{without}");
    press(&mut founder, "v").await;
    press(&mut founder, "y").await;
    assert_eq!(count(&rig, "invite_request_vouch").await, 0);
    rig.sql("INSERT INTO role_permission (role, permission) VALUES ('sysop', 'members.vouch')")
        .await;

    // members.approve taken away: the key is not offered and no account is made.
    rig.sql("DELETE FROM role_permission WHERE role = 'sysop' AND permission = 'members.approve'")
        .await;
    let without = flat(&press(&mut founder, "x").await);
    assert!(!without.contains("[a] Approve"), "{without}");
    press(&mut founder, "a").await;
    press(&mut founder, "y").await;
    assert_eq!(count(&rig, "invite_request").await, 1);
    assert_eq!(count(&rig, "accounts").await, 1, "only the founder");

    // With both decisions gone, the open screen closes on the next key with a notice.
    rig.sql("DELETE FROM role_permission WHERE role = 'sysop' AND permission = 'members.deny'")
        .await;
    let closed = flat(&press(&mut founder, "x").await);
    assert!(
        closed.contains("You are not allowed to do that."),
        "{closed}"
    );
    assert!(
        !closed.contains("[1] hopeful"),
        "the queue screen is closed\n{closed}"
    );
    assert_eq!(count(&rig, "invite_request").await, 1);
    rig.stop().await;
}
