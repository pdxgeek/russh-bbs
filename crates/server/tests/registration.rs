//! Registration modes `apply` and `open` over a real SSH client (M2-G, D2, D23, D73, D74): an
//! applicant lands in M2-E's queue and stays outside until a sysop approves; an `open` visitor
//! gets an active account at once within the per-key and per-network gates and the UTC day's
//! cap; a full day turns the board into `apply` with an honest notice; and in every mode the
//! forms answer the same words whatever the board holds. Everything uses a temporary database,
//! an injected store clock and keys generated here.
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
use russh_bbs_core::audit::{Action, AuditQuery, AuditStore, FixedClock};
use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::store::{
    AccountState, AccountStore, NewAccount, RegistrationMode, set_registration_mode,
};
use russh_bbs_core::waiting_room::RatePolicy;
use russh_bbs_server::founder;
use russh_bbs_server::ssh::{HostKey, Limits, ServerConfig, SshServer};
use russh_bbs_store::SqliteStore;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};
use tempfile::TempDir;
use tokio::time::timeout;

/// The first second of day 20,000 (UTC), the store clock's day in these tests.
const DAY: i64 = 20_000 * 86_400;

/// What an applicant reads after submitting the application form, whatever became of it.
const APPLIED: &str = "Thank you. If your application is kept";
/// What a visitor reads when an `open` attempt did not work, whatever the cause.
const NOT_CREATED: &str = "That account could not be created.";
/// The honest notice of a day whose cap is used up.
const CAPPED: &str = "Today's limit on new accounts has been reached.";

struct Rig {
    server: SshServer,
    store: SqliteStore,
    db: SqlitePool,
    token: String,
    clock: Arc<FixedClock>,
    _dir: TempDir,
}

impl Rig {
    async fn start() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bbs.db");
        let clock = Arc::new(FixedClock::new(DAY + 100));
        let store = SqliteStore::open(&path)
            .await
            .unwrap()
            .with_clock(clock.clone());
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
            clock,
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

/// The create-account form (`open`): the handle, then a password and its repeat (D94 (6)).
async fn create(channel: &mut Channel<Msg>, handle: &str) -> String {
    line(channel, handle).await;
    line(channel, "a long join password").await;
    line(channel, "a long join password").await
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
    line(channel, message).await;
    line(channel, "a long join password").await;
    line(channel, "a long join password").await
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
    common::remember(fingerprint_of(&key), account.username.as_str());
    key
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
    assert!(
        !screen.contains("SERVER-READABLE"),
        "no status row (D93): {screen}"
    );
}

async fn set_mode(rig: &Rig, mode: RegistrationMode) {
    set_registration_mode(&rig.store, mode).await.unwrap();
}

/// A stranger's key opens the waiting room in the board's current mode.
async fn visitor(rig: &Rig) -> (Handle<Client>, Channel<Msg>, PrivateKey, String) {
    stranger(rig).await
}

/// Open the application form (`apply` mode, or a full `open` day).
async fn at_apply_form(rig: &Rig) -> (Handle<Client>, Channel<Msg>, PrivateKey) {
    let (session, mut channel, key, _) = visitor(rig).await;
    let form = press(&mut channel, "a").await;
    assert!(flat(&form).contains("Apply for membership"), "{form}");
    (session, channel, key)
}

/// Open the create-account form (`open` mode).
async fn at_open_form(rig: &Rig) -> (Handle<Client>, Channel<Msg>, PrivateKey) {
    let (session, mut channel, key, _) = visitor(rig).await;
    let form = press(&mut channel, "n").await;
    assert!(flat(&form).contains("Create an account"), "{form}");
    (session, channel, key)
}

/// The home, rules and form screens of the waiting room in the current mode.
async fn pages_of(rig: &Rig, form_key: char) -> Vec<String> {
    let (_s, mut channel, _key, home) = visitor(rig).await;
    let rules = press(&mut channel, "r").await;
    press(&mut channel, "\u{1b}").await;
    let form = press(&mut channel, &form_key.to_string()).await;
    vec![home, rules, form]
}

async fn is_a_member(rig: &Rig, key: &PrivateKey) -> bool {
    rig.store
        .account_by_fingerprint(&fingerprint_of(key))
        .await
        .unwrap()
        .is_some_and(|account| account.state == AccountState::Active)
}

// ---- apply -----------------------------------------------------------------------------------

#[tokio::test]
async fn an_applicant_lands_in_the_queue_and_stays_outside_until_the_sysop_approves() {
    let rig = Rig::start().await;
    set_mode(&rig, RegistrationMode::Apply).await;

    let (_s, mut applying, key) = at_apply_form(&rig).await;
    let answer = ask(&mut applying, "hopeful", "I run a small board myself").await;
    let words = flat(&answer);
    assert!(words.contains(APPLIED), "{answer}");
    assert!(
        words.contains("This key stays outside the board"),
        "{answer}"
    );
    assert!(words.contains("Nobody is notified"), "{answer}");
    // Honest: nothing here says they are in, or that anything was sent to them.
    for claim in ["approved", "welcome", "you are in", "created"] {
        assert!(!words.to_lowercase().contains(claim), "{claim}: {answer}");
    }

    // It is a row in M2-E's queue, of the application kind, and nothing else was made.
    assert_eq!(count(&rig, "invite_request").await, 1);
    assert_eq!(
        texts(&rig, "SELECT kind || '|' || handle FROM invite_request").await,
        ["application|hopeful"]
    );
    assert_eq!(
        count(&rig, "accounts").await,
        0,
        "no account exists before a decision"
    );
    assert!(!is_a_member(&rig, &key).await);

    // Still outside: the same key reconnects to the waiting room, not a member's session.
    {
        let (_s2, _c2, _k, home) = {
            let session = login(rig.addr(), "anyone", &key).await;
            let mut channel = common::open_shell(&session, 80, 24).await;
            drain(&mut channel).await;
            channel.window_change(80, 24, 0, 0).await.unwrap();
            let home = strip_ansi(&drain(&mut channel).await);
            (session, channel, key.clone(), home)
        };
        assert!(flat(&home).contains("Waiting room"), "{home}");
        assert!(!flat(&home).contains("Signed in as"), "{home}");
    }

    // The application is audited once: by the board, the request's id, nothing typed.
    let audit =
        russh_bbs_core::audit::known_events(rig.store.page(&AuditQuery::default()).await.unwrap());
    let submitted: Vec<_> = audit
        .iter()
        .filter(|row| row.event.action == Action::ApplicationSubmitted)
        .collect();
    assert_eq!(submitted.len(), 1);
    let dump = format!("{audit:?}");
    assert!(!dump.contains("hopeful") && !dump.contains("small board"));
    assert!(!dump.contains(fingerprint_of(&key).as_str()));

    // The sysop approves it from the screen M2-E built; the key then signs in as a member.
    let (_fs, mut founder, _) = founder_at_menu(&rig).await;
    let list = open_queue(&mut founder).await;
    let words = flat(&list);
    assert!(words.contains("[1] hopeful"), "{list}");
    assert!(words.contains("I run a small board myself"), "{list}");
    assert_fits_80x24(&list);
    let detail = flat(&press(&mut founder, "1").await);
    assert!(detail.contains("Application 1: hopeful"), "{detail}");
    press(&mut founder, "a").await;
    let done = flat(&press(&mut founder, "y").await);
    assert!(done.contains("Approved."), "{done}");
    assert!(is_a_member(&rig, &key).await);
    assert_eq!(
        texts(
            &rig,
            "SELECT CAST(invited_by AS TEXT) FROM accounts WHERE username = 'hopeful'"
        )
        .await,
        ["1"]
    );
    common::remember(
        key.public_key().fingerprint(russh::keys::HashAlg::Sha256),
        "hopeful",
    );
    let session = login(rig.addr(), "anyone", &key).await;
    let mut channel = common::open_shell(&session, 80, 24).await;
    drain(&mut channel).await;
    channel.window_change(80, 24, 0, 0).await.unwrap();
    let screen = strip_ansi(&drain(&mut channel).await);
    assert!(flat(&screen).contains("Signed in as hopeful"), "{screen}");
    rig.stop().await;
}

#[tokio::test]
async fn an_applicant_reads_the_same_words_whatever_the_queue_does_with_the_application() {
    let rig = Rig::start().await;
    set_mode(&rig, RegistrationMode::Apply).await;

    let (_s1, mut first, _key1) = at_apply_form(&rig).await;
    let kept = ask(&mut first, "hopeful", "a friend of a friend").await;
    assert!(flat(&kept).contains(APPLIED), "{kept}");
    assert!(
        !kept.contains("hopeful") && !kept.contains("friend of a friend"),
        "what was typed does not stay on screen: {kept}"
    );
    assert_eq!(count(&rig, "invite_request").await, 1);

    // A repeat, a third try and a rate-limited fourth from the same key.
    for (what, handle) in [
        ("repeat", "other-name"),
        ("third", "third-name"),
        ("limited", "fourth-name"),
    ] {
        let screen = ask(&mut first, handle, "again").await;
        assert_eq!(screen, kept, "{what}");
    }
    assert_eq!(count(&rig, "invite_request").await, 1);

    // A full queue (the cap is the queue's own setting).
    rig.sql("INSERT INTO settings (key, value) VALUES ('queue_max_pending', '1')")
        .await;
    let (_s2, mut full, _) = at_apply_form(&rig).await;
    assert_eq!(
        ask(&mut full, "latecomer", "no room").await,
        kept,
        "queue full"
    );
    rig.sql("DELETE FROM settings WHERE key = 'queue_max_pending'")
        .await;

    // A key that was just denied.
    let (_s3, mut denied, denied_key) = at_apply_form(&rig).await;
    let hash = russh_bbs_core::queue::cooldown_key(&fingerprint_of(&denied_key));
    rig.sql(&format!(
        "INSERT INTO request_denial (key_hash, expires_at) VALUES ('{hash}', 99999999999)"
    ))
    .await;
    assert_eq!(
        ask(&mut denied, "denied-one", "please").await,
        kept,
        "cooling down"
    );

    // A key that already has an account that is not active.
    let suspended = member_with(&rig, "suspended-one", Slug::MEMBER, AccountState::Suspended).await;
    {
        let session = login(rig.addr(), "new", &suspended).await;
        let mut channel = common::open_shell(&session, 80, 24).await;
        drain(&mut channel).await;
        press(&mut channel, "a").await;
        assert_eq!(
            ask(&mut channel, "suspended-again", "let me back").await,
            kept,
            "has account"
        );
    }

    // The board stops taking applications while the session is open (a sysop changed the mode).
    let (_s4, mut stale, _) = at_apply_form(&rig).await;
    set_mode(&rig, RegistrationMode::Invite).await;
    assert_eq!(
        ask(&mut stale, "too-late", "mode changed").await,
        kept,
        "mode changed"
    );
    set_mode(&rig, RegistrationMode::Apply).await;

    // Without `membership.apply` (a sysop took it from the waiting role): the same words.
    rig.sql(
        "DELETE FROM role_permission WHERE role = 'waiting' AND permission = 'membership.apply'",
    )
    .await;
    let (_s5, mut gone, _) = at_apply_form(&rig).await;
    assert_eq!(
        ask(&mut gone, "no-permission", "please").await,
        kept,
        "permission not held"
    );
    rig.sql(
        "INSERT INTO role_permission (role, permission) VALUES ('waiting', 'membership.apply')",
    )
    .await;

    // Exactly the first application exists; nothing else was kept or audited.
    assert_eq!(count(&rig, "invite_request").await, 1);
    assert_eq!(
        count(&rig, "accounts").await,
        1,
        "the suspended account only"
    );
    let audit =
        russh_bbs_core::audit::known_events(rig.store.page(&AuditQuery::default()).await.unwrap());
    let submitted = audit
        .iter()
        .filter(|row| row.event.action == Action::ApplicationSubmitted)
        .count();
    assert_eq!(submitted, 1, "only the kept application is audited");
    rig.stop().await;
}

// ---- open ------------------------------------------------------------------------------------

#[tokio::test]
async fn an_open_visitor_gets_an_active_account_at_once() {
    let rig = Rig::start().await;
    set_mode(&rig, RegistrationMode::Open).await;

    let (_s, mut form, key) = at_open_form(&rig).await;
    let screen = create(&mut form, "newcomer").await;
    // The visitor is a member now: the member's login screen, then the menu.
    assert!(flat(&screen).contains("Signed in as newcomer"), "{screen}");
    assert!(is_a_member(&rig, &key).await);
    assert_eq!(
        texts(&rig, "SELECT state || '|' || CASE WHEN invited_by IS NULL THEN 'none' ELSE 'set' END FROM accounts WHERE username = 'newcomer'").await,
        ["active|none"],
        "active, and nobody vouched"
    );
    assert_eq!(count(&rig, "invite_request").await, 0, "no queue involved");
    // The same key reconnects as a member, under its new handle.
    common::remember(
        key.public_key().fingerprint(russh::keys::HashAlg::Sha256),
        "newcomer",
    );
    let session = login(rig.addr(), "anyone", &key).await;
    let mut channel = common::open_shell(&session, 80, 24).await;
    drain(&mut channel).await;
    channel.window_change(80, 24, 0, 0).await.unwrap();
    assert!(flat(&strip_ansi(&drain(&mut channel).await)).contains("Signed in as newcomer"));

    // Audit: one account.created by the board, and nothing with the key in it.
    let audit =
        russh_bbs_core::audit::known_events(rig.store.page(&AuditQuery::default()).await.unwrap());
    let created: Vec<_> = audit
        .iter()
        .filter(|row| row.event.action == Action::AccountCreated)
        .collect();
    assert_eq!(created.len(), 1);
    assert_eq!(created[0].event.detail.as_str(), "via open state active");
    assert!(!format!("{audit:?}").contains(fingerprint_of(&key).as_str()));
    // The day's counter has one.
    assert_eq!(
        texts(&rig, "SELECT CAST(opened AS TEXT) FROM registration_day").await,
        ["1"]
    );
    rig.stop().await;
}

#[tokio::test]
async fn an_open_refusal_reads_the_same_whatever_the_cause() {
    let rig = Rig::start().await;
    set_mode(&rig, RegistrationMode::Open).await;
    member_with(&rig, "alice", Slug::MEMBER, AccountState::Active).await;

    // The baseline refusal: a handle that is taken.
    let (_s1, mut taken, _k1) = at_open_form(&rig).await;
    let baseline = create(&mut taken, "alice").await;
    assert!(flat(&baseline).contains(NOT_CREATED), "{baseline}");
    assert!(
        !baseline.contains("alice"),
        "what was typed does not stay: {baseline}"
    );

    // A lookalike of a taken handle.
    let (_s2, mut lookalike, _k2) = at_open_form(&rig).await;
    assert_eq!(create(&mut lookalike, "a1ice").await, baseline, "lookalike");

    // A key that already has a pending account (it still reaches the room).
    let pending = member_with(&rig, "pending-one", Slug::MEMBER, AccountState::Pending).await;
    let accounts_before = count(&rig, "accounts").await;
    {
        let session = login(rig.addr(), "new", &pending).await;
        let mut channel = common::open_shell(&session, 80, 24).await;
        drain(&mut channel).await;
        press(&mut channel, "n").await;
        assert_eq!(
            create(&mut channel, "brand-new").await,
            baseline,
            "key has an account"
        );
    }

    // The per-key gate: a burst of two attempts, then none for an hour, however good the
    // handle. The key makes two failed attempts, then a third with a free handle.
    let (_s3, mut limited, key3) = at_open_form(&rig).await;
    assert_eq!(create(&mut limited, "alice").await, baseline);
    assert_eq!(create(&mut limited, "alice").await, baseline);
    assert_eq!(
        create(&mut limited, "free-handle").await,
        baseline,
        "rate limited"
    );
    assert!(!is_a_member(&rig, &key3).await);

    // Without `membership.open`: the same words and nothing made.
    rig.sql(
        "DELETE FROM role_permission WHERE role = 'waiting' AND permission = 'membership.open'",
    )
    .await;
    let (_s4, mut gone, key4) = at_open_form(&rig).await;
    assert_eq!(
        create(&mut gone, "free-handle").await,
        baseline,
        "permission not held"
    );
    assert!(!is_a_member(&rig, &key4).await);
    rig.sql("INSERT INTO role_permission (role, permission) VALUES ('waiting', 'membership.open')")
        .await;

    // The board leaves `open` while the form is open (a sysop changed the mode).
    let (_s5, mut stale, key5) = at_open_form(&rig).await;
    set_mode(&rig, RegistrationMode::Invite).await;
    assert_eq!(
        create(&mut stale, "free-handle").await,
        baseline,
        "mode changed"
    );
    assert!(!is_a_member(&rig, &key5).await);
    set_mode(&rig, RegistrationMode::Open).await;

    // Nothing was made by any of it, and nothing was audited.
    assert_eq!(count(&rig, "accounts").await, accounts_before);
    assert_eq!(count(&rig, "registration_day").await, 0);
    let audit =
        russh_bbs_core::audit::known_events(rig.store.page(&AuditQuery::default()).await.unwrap());
    assert!(
        audit
            .iter()
            .all(|row| !row.event.detail.as_str().contains("via open")),
        "no open registration was audited"
    );
    rig.stop().await;
}

#[tokio::test]
async fn the_network_gate_limits_a_flood_of_new_keys_from_one_address() {
    // Five accounts from one address (all keys here come from 127.0.0.1), then none until the
    // gate refills; each refusal reads like any other.
    let rig = Rig::start().await;
    set_mode(&rig, RegistrationMode::Open).await;
    rig.sql("INSERT INTO settings (key, value) VALUES ('open_daily_cap', '0')")
        .await;
    for n in 1..=5 {
        let (_s, mut form, key) = at_open_form(&rig).await;
        let screen = create(&mut form, &format!("person{n}")).await;
        assert!(
            flat(&screen).contains(&format!("Signed in as person{n}")),
            "{n}: {screen}"
        );
        assert!(is_a_member(&rig, &key).await);
    }
    let (_s, mut form, key) = at_open_form(&rig).await;
    let screen = create(&mut form, "person6").await;
    assert!(flat(&screen).contains(NOT_CREATED), "{screen}");
    assert!(!is_a_member(&rig, &key).await);
    assert_eq!(count(&rig, "accounts").await, 5);
    rig.stop().await;
}

// ---- the daily cap ---------------------------------------------------------------------------

#[tokio::test]
async fn the_daily_cap_degrades_open_to_apply_for_the_rest_of_the_utc_day() {
    let rig = Rig::start().await;
    set_mode(&rig, RegistrationMode::Open).await;
    rig.sql("INSERT INTO settings (key, value) VALUES ('open_daily_cap', '2')")
        .await;

    // A visitor opens the create-account form while the day still has room...
    let (_late_session, mut late, late_key) = at_open_form(&rig).await;
    // ...two others take the day's places...
    for n in 1..=2 {
        let (_s, mut form, _) = at_open_form(&rig).await;
        let screen = create(&mut form, &format!("person{n}")).await;
        assert!(
            flat(&screen).contains(&format!("Signed in as person{n}")),
            "{screen}"
        );
    }
    // ...and theirs is refused with the one sentence, and nothing is made.
    let refused = create(&mut late, "latecomer").await;
    assert!(flat(&refused).contains(NOT_CREATED), "{refused}");
    assert!(!is_a_member(&rig, &late_key).await);
    assert_eq!(count(&rig, "accounts").await, 2);
    // Leaving the form shows the apply room and its notice on the same connection (FIX-M2-4).
    let left = press(&mut late, "\u{1b}").await;
    let words = flat(&left);
    assert!(words.contains(CAPPED), "{left}");
    assert!(words.contains("[a] Apply for membership"), "{left}");
    assert!(!words.contains("Create an account"), "{left}");

    // The next connection sees the apply screens with an honest notice instead of the form.
    let (_s, mut channel, key, home) = visitor(&rig).await;
    let words = flat(&home);
    assert!(words.contains(CAPPED), "{home}");
    assert!(words.contains("[a] Apply for membership"), "{home}");
    assert!(!words.contains("Create an account"), "{home}");
    let form = press(&mut channel, "a").await;
    assert!(flat(&form).contains("Apply for membership"), "{form}");
    assert!(flat(&form).contains(CAPPED), "{form}");
    // The create-account key does nothing now.
    press(&mut channel, "\u{1b}").await;
    let after = press(&mut channel, "n").await;
    assert!(flat(&after).contains("Waiting room"), "{after}");
    // The form is the apply form: the answer is the apply answer, and it queues.
    press(&mut channel, "a").await;
    let answer = ask(&mut channel, "applicant", "the board was full today").await;
    assert!(flat(&answer).contains(APPLIED), "{answer}");
    assert_eq!(
        texts(&rig, "SELECT kind FROM invite_request").await,
        ["application"]
    );
    assert!(!is_a_member(&rig, &key).await);

    // The audit log has the one `registration.capped` row, however many were turned away.
    let audit =
        russh_bbs_core::audit::known_events(rig.store.page(&AuditQuery::default()).await.unwrap());
    let capped = audit
        .iter()
        .filter(|row| row.event.action == Action::RegistrationCapped)
        .count();
    assert_eq!(capped, 1);

    // The last second of the day is still capped; the first of the next day is open again.
    rig.clock.set(DAY + 86_399);
    let (_s, _c, _k, home) = visitor(&rig).await;
    assert!(flat(&home).contains(CAPPED), "{home}");
    rig.clock.set(DAY + 86_400);
    let (_s, mut fresh, fresh_key, home) = visitor(&rig).await;
    let words = flat(&home);
    assert!(!words.contains(CAPPED), "{home}");
    assert!(words.contains("[n] Create an account"), "{home}");
    let screen = {
        press(&mut fresh, "n").await;
        create(&mut fresh, "nextday").await
    };
    assert!(flat(&screen).contains("Signed in as nextday"), "{screen}");
    assert!(is_a_member(&rig, &fresh_key).await);
    rig.stop().await;
}

#[tokio::test]
async fn a_cap_of_zero_means_no_cap_and_a_bad_value_means_the_default() {
    let rig = Rig::start().await;
    set_mode(&rig, RegistrationMode::Open).await;
    // A bad value is the default (25), not "no cap": with 25 made today the board is full.
    rig.sql("INSERT INTO settings (key, value) VALUES ('open_daily_cap', 'lots')")
        .await;
    rig.sql(&format!(
        "INSERT INTO registration_day (day, opened) VALUES ({}, 25)",
        DAY / 86_400
    ))
    .await;
    let (_s, _c, _k, home) = visitor(&rig).await;
    assert!(flat(&home).contains(CAPPED), "{home}");
    // Zero is the deliberate way to say no cap.
    rig.sql("UPDATE settings SET value = '0' WHERE key = 'open_daily_cap'")
        .await;
    let (_s, _c, _k, home) = visitor(&rig).await;
    assert!(!flat(&home).contains(CAPPED), "{home}");
    assert!(flat(&home).contains("[n] Create an account"), "{home}");
    rig.stop().await;
}

// ---- the same screens in every mode -----------------------------------------------------------

#[tokio::test]
async fn the_waiting_room_shows_the_same_screens_whatever_the_board_holds_in_every_mode() {
    let rig = Rig::start().await;
    for (mode, form_key) in [
        (RegistrationMode::Apply, 'a'),
        (RegistrationMode::Open, 'n'),
    ] {
        set_mode(&rig, mode).await;
        let empty = pages_of(&rig, form_key).await;

        // A board with members and staff, a queue with applications and vouches, a denial, and
        // some of the open day's places used (but not the cap).
        for (n, role) in [(1, Slug::MEMBER), (2, Slug::TRUSTED), (3, Slug::MODERATOR)] {
            member_with(
                &rig,
                &format!("someone{n}-{mode:?}"),
                role,
                AccountState::Active,
            )
            .await;
        }
        for n in 1..=5 {
            rig.sql(&format!(
                "INSERT OR IGNORE INTO invite_request (fingerprint, handle, message, created_at, expires_at, kind) \
                 VALUES ('SHA256:{n:A>43}', 'person{n}', 'hello', 100, 99999999999, 'application')"
            ))
            .await;
        }
        rig.sql(&format!(
            "INSERT OR REPLACE INTO registration_day (day, opened) VALUES ({}, 7)",
            DAY / 86_400
        ))
        .await;
        rig.sql(&format!(
            "INSERT OR REPLACE INTO request_denial (key_hash, expires_at) VALUES ('{}', 99999999999)",
            "0".repeat(64)
        ))
        .await;
        let busy = pages_of(&rig, form_key).await;
        // A full queue.
        rig.sql("INSERT OR REPLACE INTO settings (key, value) VALUES ('queue_max_pending', '5')")
            .await;
        let full = pages_of(&rig, form_key).await;
        rig.sql("DELETE FROM settings WHERE key = 'queue_max_pending'")
            .await;

        assert_eq!(
            empty, busy,
            "{mode:?}: members, requests and the day's count change nothing"
        );
        assert_eq!(empty, full, "{mode:?}: a full queue changes nothing");
        for page in &empty {
            let lower = page.to_lowercase();
            for word in [
                "queue", "pending", "person1", "someone", "members:", "boards:", "sysop",
                "founder", "today", "limit",
            ] {
                assert!(!lower.contains(word), "{mode:?}: {word:?} leaks:\n{page}");
            }
        }
    }
    rig.stop().await;
}

#[tokio::test]
async fn a_mode_change_is_picked_up_by_the_next_connection_without_a_restart() {
    let rig = Rig::start().await;
    let menu_of = |home: &str| {
        let words = flat(home);
        (
            words.contains("[i] Request an invite"),
            words.contains("[c] Redeem an invite code"),
            words.contains("[a] Apply for membership"),
            words.contains("[n] Create an account"),
        )
    };
    let (_s, _c, _k, home) = visitor(&rig).await;
    assert_eq!(
        menu_of(&home),
        (true, true, false, false),
        "the default is invite"
    );
    set_mode(&rig, RegistrationMode::Apply).await;
    let (_s, _c, _k, home) = visitor(&rig).await;
    assert_eq!(menu_of(&home), (false, false, true, false));
    set_mode(&rig, RegistrationMode::Open).await;
    let (_s, _c, _k, home) = visitor(&rig).await;
    assert_eq!(menu_of(&home), (false, false, false, true));
    set_mode(&rig, RegistrationMode::Invite).await;
    let (_s, _c, _k, home) = visitor(&rig).await;
    assert_eq!(menu_of(&home), (true, true, false, false));
    // A value the board does not know falls back to the strictest set, never a looser one.
    rig.sql("UPDATE settings SET value = 'wide-open' WHERE key = 'registration_mode'")
        .await;
    let (_s, _c, _k, home) = visitor(&rig).await;
    assert_eq!(menu_of(&home), (true, true, false, false));
    rig.stop().await;
}
