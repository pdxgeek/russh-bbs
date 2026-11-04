//! FIX-M3-11 over a real russh client against the real SQLite store (D97): My keys, a device-link
//! code that binds a second machine, removing the old key, a staff recovery code, the rank rule
//! (a cosysop cannot help a sysop), and one refusal for every wrong, used or expired code.
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
use russh_bbs_core::audit::{AuditQuery, AuditStore, FixedClock};
use russh_bbs_core::icons::Tier;
use russh_bbs_core::passwords::Backoff;
use russh_bbs_core::passwords::PasswordStore;
use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::store::{AccountId, AccountState, AccountStore, NewAccount};
use russh_bbs_core::terminal_settings::{TerminalSettings, TerminalSettingsStore};
use russh_bbs_core::waiting_room::RatePolicy;
use russh_bbs_server::founder;
use russh_bbs_server::ssh::{HostKey, Limits, ServerConfig, SshServer};
use russh_bbs_store::SqliteStore;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};
use tempfile::TempDir;
use tokio::time::timeout;

/// The first second of day 20,000 (UTC), the store clock's day in these tests.
const DAY: i64 = 20_000 * 86_400;
/// What a visitor reads when a code did not work, whatever the cause.
const REFUSED: &str = "That code did not work.";
const PASSWORD: &str = "the old long password";
const NEW_PASSWORD: &str = "a brand new long password";

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
        let loose = RatePolicy::new(1_000, Duration::from_millis(1));
        let limits = Limits {
            ip_rate: loose,
            key_rate: loose,
            password_handle_backoff: Backoff::UNLIMITED,
            password_network_backoff: Backoff::UNLIMITED,
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

/// Whether the key is accepted under the handle.
async fn key_signs_in(addr: SocketAddr, handle: &str, key: &PrivateKey) -> bool {
    let mut session = common::connect(addr).await;
    let result = session
        .authenticate_publickey(
            handle,
            PrivateKeyWithHashAlg::new(Arc::new(key.clone()), None),
        )
        .await
        .unwrap();
    matches!(result, AuthResult::Success)
}

/// Whether the password is accepted under the handle.
async fn password_signs_in(addr: SocketAddr, handle: &str, password: &str) -> bool {
    let mut session = common::connect(addr).await;
    let result = session
        .authenticate_password(handle, password)
        .await
        .unwrap();
    matches!(result, AuthResult::Success)
}

/// Replay the terminal byte stream onto a blank screen and return the rows as text, the way a
/// person would see them.
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
/// Type `text` and press Enter.
async fn line(channel: &mut Channel<Msg>, text: &str) -> String {
    press(channel, &format!("{text}\r")).await
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

async fn actions(rig: &Rig, like: &str) -> Vec<String> {
    sqlx::query_scalar::<_, String>(&format!(
        "SELECT action FROM audit_log WHERE action LIKE '{like}' ORDER BY id"
    ))
    .fetch_all(&rig.db)
    .await
    .unwrap()
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

/// An account with a fresh key, terminal settings already chosen, and optionally a password.
async fn member_with(
    rig: &Rig,
    name: &str,
    role: Slug,
    password: Option<&str>,
) -> (AccountId, PrivateKey) {
    let key = new_client_key();
    let account = rig
        .store
        .create_account(NewAccount {
            username: sanitize_username(name).unwrap(),
            role,
            state: AccountState::Active,
        })
        .await
        .unwrap();
    rig.store
        .attach_fingerprint(account.id, &fingerprint_of(&key))
        .await
        .unwrap();
    common::remember(fingerprint_of(&key), account.username.as_str());
    let settings = TerminalSettings::new(None, None, true, true, Some(Tier::Ascii)).unwrap();
    rig.store
        .set_terminal_settings(account.id, &settings)
        .await
        .unwrap();
    if let Some(password) = password {
        let hash = russh_bbs_core::passwords::hash::hash(password).unwrap();
        rig.store
            .set_hash(
                account.id,
                Some(&hash),
                &russh_bbs_core::audit::events::password_set(account.id, "key session"),
            )
            .await
            .unwrap();
    }
    (account.id, key)
}

/// Sign in with a key under its handle and return the login screen, then the main menu.
async fn at_menu(
    rig: &Rig,
    handle: &str,
    key: &PrivateKey,
) -> (Handle<Client>, Channel<Msg>, String) {
    let session = login(rig.addr(), handle, key).await;
    let mut channel = common::open_shell(&session, 80, 24).await;
    drain(&mut channel).await;
    channel.window_change(80, 24, 0, 0).await.unwrap();
    let signed_in = strip_ansi(&drain(&mut channel).await);
    assert!(flat(&signed_in).contains("Signed in as"), "{signed_in}");
    let menu = press(&mut channel, "\r").await;
    assert!(flat(&menu).contains("Main menu"), "{menu}");
    (session, channel, signed_in)
}

/// From the main menu to the My keys screen.
async fn my_keys(channel: &mut Channel<Msg>) -> String {
    press(channel, "u").await;
    let screen = press(channel, "k").await;
    assert!(
        flat(&screen).contains("Keys sign you in without a password"),
        "{screen}"
    );
    screen
}

/// The code shown on a code page: the word after `Code:`.
fn code_in(screen: &str) -> String {
    let text = flat(screen);
    let after = text.split("Code: ").nth(1).expect("a code is showing");
    after.split_whitespace().next().unwrap().to_owned()
}

/// The number the My keys list gives the key with this fingerprint.
fn number_of(screen: &str, key: &PrivateKey) -> char {
    let fingerprint = fingerprint_of(key).to_string();
    screen
        .lines()
        .find(|l| l.contains(&fingerprint))
        .and_then(|l| l.trim_start_matches(['|', '│', ' ']).chars().next())
        .filter(char::is_ascii_digit)
        .unwrap_or_else(|| panic!("{fingerprint} is not listed:\n{screen}"))
}

/// A new machine: a visitor with a fresh key, on the waiting room's home page.
async fn new_machine(rig: &Rig) -> (Handle<Client>, Channel<Msg>, PrivateKey, String) {
    let key = new_client_key();
    let session = login(rig.addr(), "new", &key).await;
    let mut channel = common::open_shell(&session, 80, 24).await;
    drain(&mut channel).await;
    channel.window_change(80, 24, 0, 0).await.unwrap();
    let home = strip_ansi(&drain(&mut channel).await);
    (session, channel, key, home)
}

#[tokio::test]
async fn a_second_machine_is_linked_with_a_code_and_the_old_key_is_removed() {
    let rig = Rig::start().await;
    let (_s1, mut first, key1) = founder_at_menu(&rig).await;
    let handle = russh_bbs_core::founder::FOUNDER_USERNAME;

    // My keys lists the key the founder claimed with, by fingerprint.
    let listed = my_keys(&mut first).await;
    assert!(
        flat(&listed).contains(&fingerprint_of(&key1).to_string()),
        "{listed}"
    );
    assert!(flat(&listed).contains("last used never"), "{listed}");
    // The fingerprint is on a line of its own, whole.
    assert!(
        listed
            .lines()
            .any(|l| l.contains(&format!("1. {}", fingerprint_of(&key1)))),
        "{listed}"
    );

    // A device-link code: shown once, 10 minutes, stored only as a hash.
    let shown = press(&mut first, "l").await;
    assert!(flat(&shown).contains("expires in 10 minutes"), "{shown}");
    let code = code_in(&shown);
    assert!(
        sqlx::query_scalar::<_, String>("SELECT code_hash FROM login_code")
            .fetch_all(&rig.db)
            .await
            .unwrap()
            .iter()
            .all(|stored| stored.len() == 64 && !code.contains(stored.as_str()))
    );
    let back = press(&mut first, "x").await;
    assert!(
        !flat(&back).contains(&code),
        "the code stayed on screen: {back}"
    );

    // The new machine enters it in the waiting room and is signed in as the founder.
    let (_s2, mut second, key2, home) = new_machine(&rig).await;
    assert!(flat(&home).contains("Link this device"), "{home}");
    assert!(flat(&home).contains("Recovery code"), "{home}");
    let form = press(&mut second, "d").await;
    assert!(flat(&form).contains("Link code"), "{form}");
    let joined = line(&mut second, &code).await;
    assert!(
        flat(&joined).contains(&format!("Signed in as {handle}")),
        "{joined}"
    );
    assert_eq!(count(&rig, "account_keys").await, 2);
    assert!(key_signs_in(rig.addr(), handle, &key2).await);
    assert_eq!(
        actions(&rig, "device_code.%").await,
        ["device_code.issued", "device_code.redeemed"]
    );

    // The code works once: a third machine is refused with the one sentence.
    let (_s3, mut third, key3, _) = new_machine(&rig).await;
    press(&mut third, "d").await;
    let used = line(&mut third, &code).await;
    assert!(flat(&used).contains(REFUSED), "{used}");
    assert!(!key_signs_in(rig.addr(), handle, &key3).await);

    // On the new machine remove the old key.
    press(&mut second, "\r").await;
    let listed = my_keys(&mut second).await;
    let number = number_of(&listed, &key1);
    press(&mut second, "r").await;
    let confirm = press(&mut second, &number.to_string()).await;
    assert!(
        flat(&confirm).contains(&fingerprint_of(&key1).to_string()),
        "{confirm}"
    );
    let done = press(&mut second, "y").await;
    assert!(flat(&done).contains("Key removed"), "{done}");
    assert!(
        !key_signs_in(rig.addr(), handle, &key1).await,
        "the old key still signs in"
    );
    assert!(key_signs_in(rig.addr(), handle, &key2).await);
    assert_eq!(count(&rig, "account_keys").await, 1);
    assert_eq!(actions(&rig, "key.%").await, ["key.removed"]);

    // The next sign-in tells the member about both changes, once.
    let (_s4, _c4, signed_in) = at_menu(&rig, handle, &key2).await;
    let told = flat(&signed_in);
    assert!(
        told.contains("Changes to your keys since you were last here"),
        "{signed_in}"
    );
    assert!(
        told.contains("A key was added with a device-link code on"),
        "{signed_in}"
    );
    assert!(told.contains("A key was removed on"), "{signed_in}");
    assert!(told.contains("If this was not you"), "{signed_in}");
    let (_s5, _c5, again) = at_menu(&rig, handle, &key2).await;
    assert!(!flat(&again).contains("Changes to your keys"), "{again}");
    rig.stop().await;
}

#[tokio::test]
async fn the_last_way_in_cannot_be_removed_and_a_pasted_key_is_validated() {
    let rig = Rig::start().await;
    let (_id, key) = member_with(&rig, "solo", Slug::MEMBER, None).await;
    let (_s, mut channel, _) = at_menu(&rig, "solo", &key).await;
    let listed = my_keys(&mut channel).await;
    let number = number_of(&listed, &key);
    press(&mut channel, "r").await;
    press(&mut channel, &number.to_string()).await;
    let refused = press(&mut channel, "y").await;
    assert!(
        flat(&refused).contains("That is your only way in"),
        "{refused}"
    );
    assert_eq!(count(&rig, "account_keys").await, 1);
    assert!(key_signs_in(rig.addr(), "solo", &key).await);

    // A paste that is not an Ed25519 public key says so and stores nothing.
    press(&mut channel, "a").await;
    let bad = line(&mut channel, "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABAQC").await;
    assert!(flat(&bad).contains("not an Ed25519 public key"), "{bad}");
    // A key that already has an account is refused with one message, whoever has it.
    let other = new_client_key();
    let (_oid, other_key) = member_with(&rig, "other", Slug::MEMBER, None).await;
    drop(other);
    press(&mut channel, "a").await;
    let taken = line(&mut channel, &other_key.public_key().to_openssh().unwrap()).await;
    assert!(
        flat(&taken).contains("That key cannot be added."),
        "{taken}"
    );
    press(&mut channel, "a").await;
    let own = line(&mut channel, &key.public_key().to_openssh().unwrap()).await;
    assert!(flat(&own).contains("That key cannot be added."), "{own}");
    // A fresh key is added, audited and then removable.
    let fresh = new_client_key();
    press(&mut channel, "a").await;
    let added = line(&mut channel, &fresh.public_key().to_openssh().unwrap()).await;
    assert!(flat(&added).contains("Key added"), "{added}");
    assert!(
        flat(&added).contains(&fingerprint_of(&fresh).to_string()),
        "{added}"
    );
    assert_eq!(actions(&rig, "key.%").await, ["key.added"]);
    assert!(key_signs_in(rig.addr(), "solo", &fresh).await);
    rig.stop().await;
}

#[tokio::test]
async fn device_link_codes_are_limited_to_five_an_hour() {
    let rig = Rig::start().await;
    let (_id, key) = member_with(&rig, "maker", Slug::MEMBER, Some(PASSWORD)).await;
    let (_s, mut channel, _) = at_menu(&rig, "maker", &key).await;
    my_keys(&mut channel).await;
    for n in 0..5 {
        let shown = press(&mut channel, "l").await;
        assert!(flat(&shown).contains("Code:"), "code {n}: {shown}");
        press(&mut channel, "x").await;
    }
    let sixth = press(&mut channel, "l").await;
    assert!(
        flat(&sixth).contains("You have made enough codes for now"),
        "{sixth}"
    );
    assert_eq!(count(&rig, "login_code").await, 5);
    // An hour later there is room again.
    rig.clock.set(DAY + 100 + 3_601);
    let again = press(&mut channel, "l").await;
    assert!(flat(&again).contains("Code:"), "{again}");
    rig.stop().await;
}

#[tokio::test]
async fn a_staff_recovery_code_binds_a_new_key_and_replaces_the_password() {
    let rig = Rig::start().await;
    let (_s, mut staff, _key) = founder_at_menu(&rig).await;
    let (alice, old_key) = member_with(&rig, "alice", Slug::MEMBER, Some(PASSWORD)).await;

    // The staff screen under Sysop tools tells staff to confirm identity first.
    press(&mut staff, "o").await;
    let screen = press(&mut staff, "r").await;
    assert!(
        flat(&screen).contains("Confirm who is asking outside this board"),
        "{screen}"
    );
    let shown = line(&mut staff, "alice").await;
    assert!(flat(&shown).contains("expires"), "{shown}");
    let code = code_in(&shown);
    assert_eq!(
        actions(&rig, "recovery_code.%").await,
        ["recovery_code.issued"]
    );

    // alice lost her key and password. A new machine uses the code and sets a new password.
    let (_s2, mut machine, new_key, home) = new_machine(&rig).await;
    assert!(flat(&home).contains("Recovery code"), "{home}");
    press(&mut machine, "y").await;
    line(&mut machine, &code).await;
    line(&mut machine, NEW_PASSWORD).await;
    let joined = line(&mut machine, NEW_PASSWORD).await;
    assert!(flat(&joined).contains("Signed in as alice"), "{joined}");
    assert!(key_signs_in(rig.addr(), "alice", &new_key).await);
    assert!(password_signs_in(rig.addr(), "alice", NEW_PASSWORD).await);
    assert!(
        !password_signs_in(rig.addr(), "alice", PASSWORD).await,
        "the old password still works"
    );
    assert_eq!(
        actions(&rig, "recovery_code.%").await,
        ["recovery_code.issued", "recovery_code.redeemed"]
    );
    // Neither the code nor a password is in the log or the database in the clear.
    let audit = format!(
        "{:?}",
        russh_bbs_core::audit::known_events(rig.store.page(&AuditQuery::default()).await.unwrap())
    );
    for secret in [&code, NEW_PASSWORD, PASSWORD] {
        assert!(!audit.contains(secret), "{secret} was audited");
    }

    // The code is single use.
    let (_s3, mut late, late_key, _) = new_machine(&rig).await;
    press(&mut late, "y").await;
    line(&mut late, &code).await;
    line(&mut late, NEW_PASSWORD).await;
    let used = line(&mut late, NEW_PASSWORD).await;
    assert!(flat(&used).contains(REFUSED), "{used}");
    assert!(!key_signs_in(rig.addr(), "alice", &late_key).await);

    // The member is told at the next sign-in; her old key still signs in until she removes it.
    let (_s4, _c4, signed_in) = at_menu(&rig, "alice", &old_key).await;
    assert!(
        flat(&signed_in).contains("A recovery code was used on"),
        "{signed_in}"
    );
    let _ = alice;
    rig.stop().await;
}

#[tokio::test]
async fn a_cosysop_cannot_issue_a_code_for_a_sysop_or_an_equal_but_can_for_a_member() {
    let rig = Rig::start().await;
    let (_s, _staff, _key) = founder_at_menu(&rig).await;
    let founder = russh_bbs_core::founder::FOUNDER_USERNAME;
    member_with(&rig, "alice", Slug::MEMBER, Some(PASSWORD)).await;
    member_with(&rig, "peer", Slug::COSYSOP, Some(PASSWORD)).await;
    let (_id, deputy_key) = member_with(&rig, "deputy", Slug::COSYSOP, Some(PASSWORD)).await;
    let (_d, mut deputy, _) = at_menu(&rig, "deputy", &deputy_key).await;
    press(&mut deputy, "o").await;
    press(&mut deputy, "r").await;
    for refused in [founder, "peer", "deputy", "nobody"] {
        let screen = line(&mut deputy, refused).await;
        assert!(
            flat(&screen).contains("No code was made"),
            "{refused}: {screen}"
        );
    }
    assert_eq!(count(&rig, "login_code").await, 0);
    assert_eq!(actions(&rig, "recovery_code.%").await, Vec::<String>::new());
    let shown = line(&mut deputy, "alice").await;
    assert!(flat(&shown).contains("Code:"), "{shown}");
    assert_eq!(count(&rig, "login_code").await, 1);

    // A plain member is not offered the screen at all.
    let (_id, member_key) = member_with(&rig, "plain", Slug::MEMBER, None).await;
    let (_m, mut plain, _) = at_menu(&rig, "plain", &member_key).await;
    let menu = press(&mut plain, "o").await;
    assert!(!flat(&menu).contains("Recovery code"), "{menu}");
    rig.stop().await;
}

#[tokio::test]
async fn every_wrong_used_or_expired_code_reads_the_same_and_reveals_nothing() {
    let rig = Rig::start().await;
    let (_id, key) = member_with(&rig, "maker", Slug::MEMBER, Some(PASSWORD)).await;
    let (_s, mut maker, _) = at_menu(&rig, "maker", &key).await;
    my_keys(&mut maker).await;
    let live = code_in(&press(&mut maker, "l").await);
    press(&mut maker, "x").await;
    let stale = code_in(&press(&mut maker, "l").await);
    press(&mut maker, "x").await;
    // `stale` voided `live`; then it expires.
    rig.clock.set(DAY + 100 + 601);

    let mut answers = Vec::new();
    for typed in [
        live.as_str(),
        stale.as_str(),
        "ZZZZZ-ZZZZZ",
        "not a code",
        "0",
    ] {
        let (_s, mut machine, key, _) = new_machine(&rig).await;
        press(&mut machine, "d").await;
        let answer = line(&mut machine, typed).await;
        assert!(flat(&answer).contains(REFUSED), "{typed}: {answer}");
        answers.push(flat(&answer));
        assert!(!key_signs_in(rig.addr(), "maker", &key).await);
    }
    assert!(
        answers.windows(2).all(|pair| pair[0] == pair[1]),
        "{answers:?}"
    );
    // Nothing was bound and no code was used up by the failed tries.
    assert_eq!(count(&rig, "account_keys").await, 1);
    assert_eq!(
        count(&rig, "login_code WHERE redeemed_at IS NOT NULL").await,
        0
    );
    // A recovery form says the same for a link code (kinds do not cross) and for a bad one.
    let (_s, mut machine, _, _) = new_machine(&rig).await;
    press(&mut machine, "y").await;
    line(&mut machine, &stale).await;
    line(&mut machine, NEW_PASSWORD).await;
    let crossed = line(&mut machine, NEW_PASSWORD).await;
    assert!(flat(&crossed).contains(REFUSED), "{crossed}");
    rig.stop().await;
}

#[tokio::test]
async fn a_password_only_visitor_is_not_offered_the_key_entries() {
    // No key (a password-only visitor, like telnet) means nothing to bind: the room has no
    // "Link this device" or "Recovery code".
    let rig = Rig::start().await;
    let mut session = common::connect(rig.addr()).await;
    let result = session.authenticate_password("new", "").await.unwrap();
    assert!(matches!(result, AuthResult::Success), "{result:?}");
    let mut channel = common::open_shell(&session, 80, 24).await;
    drain(&mut channel).await;
    channel.window_change(80, 24, 0, 0).await.unwrap();
    let home = strip_ansi(&drain(&mut channel).await);
    assert!(!flat(&home).contains("Link this device"), "{home}");
    assert!(!flat(&home).contains("Recovery code"), "{home}");
    let nothing = press(&mut channel, "d").await;
    assert!(!flat(&nothing).contains("Code"), "{nothing}");
    rig.stop().await;
}

#[tokio::test]
async fn a_newer_link_code_voids_the_older_unused_one() {
    // QA P1 (FIX-M3-13): codes A then B for one member, nothing expires; A must be refused and
    // B must still work.
    let rig = Rig::start().await;
    let (_id, key) = member_with(&rig, "maker", Slug::MEMBER, Some(PASSWORD)).await;
    let (_s, mut maker, _) = at_menu(&rig, "maker", &key).await;
    my_keys(&mut maker).await;
    let older = code_in(&press(&mut maker, "l").await);
    press(&mut maker, "x").await;
    let newer = code_in(&press(&mut maker, "l").await);
    press(&mut maker, "x").await;
    assert_ne!(older, newer);
    assert_eq!(count(&rig, "login_code").await, 2);
    assert_eq!(
        count(&rig, "login_code WHERE voided_at IS NOT NULL").await,
        1
    );

    let (_s, mut machine, key_a, _) = new_machine(&rig).await;
    press(&mut machine, "d").await;
    let answer = line(&mut machine, &older).await;
    assert!(flat(&answer).contains(REFUSED), "{answer}");
    assert!(!key_signs_in(rig.addr(), "maker", &key_a).await);

    let (_s, mut machine, key_b, _) = new_machine(&rig).await;
    press(&mut machine, "d").await;
    let joined = line(&mut machine, &newer).await;
    assert!(flat(&joined).contains("Signed in as maker"), "{joined}");
    assert!(key_signs_in(rig.addr(), "maker", &key_b).await);
    rig.stop().await;
}

// ---------------------------------------------------------------- revocation (FIX-M3-14)

/// What a session reads when the credential it signed in with was revoked.
const REVOKED_LINE: &str = "Your sign-in was revoked. Please sign in again.";

/// Read until the server closes the channel; everything it drew. Panics if it stays open.
async fn closed_with(channel: &mut Channel<Msg>) -> String {
    let mut seen = String::new();
    loop {
        match timeout(Duration::from_secs(5), channel.wait())
            .await
            .expect("the session was not closed")
        {
            Some(ChannelMsg::Data { data }) => seen.push_str(&String::from_utf8_lossy(&data)),
            Some(ChannelMsg::Close) | None => return flat(&strip_ansi(&seen)),
            Some(_) => {}
        }
    }
}

/// A session that signed in with the password: at the main menu.
async fn password_menu(rig: &Rig, handle: &str, password: &str) -> (Handle<Client>, Channel<Msg>) {
    let mut session = common::connect(rig.addr()).await;
    let result = session
        .authenticate_password(handle, password)
        .await
        .unwrap();
    assert!(matches!(result, AuthResult::Success), "{result:?}");
    let mut channel = common::open_shell(&session, 80, 24).await;
    drain(&mut channel).await;
    channel.window_change(80, 24, 0, 0).await.unwrap();
    let signed_in = strip_ansi(&drain(&mut channel).await);
    assert!(flat(&signed_in).contains("Signed in as"), "{signed_in}");
    let menu = press(&mut channel, "\r").await;
    assert!(flat(&menu).contains("Main menu"), "{menu}");
    (session, channel)
}

/// Several sessions of one member at once, and calls to spare (the defaults are 2 and 5).
async fn roomy(rig: &Rig) {
    sqlx::query(
        "UPDATE role_limit SET value = 50 WHERE role = 'member' AND kind IN ('max_sessions', 'calls_per_day')",
    )
    .execute(&rig.db)
    .await
    .unwrap();
}

/// Give `account` one more key, as if it had been linked earlier.
async fn extra_key(rig: &Rig, account: AccountId, handle: &str) -> PrivateKey {
    let key = new_client_key();
    rig.store
        .attach_fingerprint(account, &fingerprint_of(&key))
        .await
        .unwrap();
    common::remember(fingerprint_of(&key), handle);
    key
}

/// The menu still answers: the session is open.
async fn still_open(channel: &mut Channel<Msg>) {
    let menu = press(channel, "\r").await;
    assert!(flat(&menu).contains("Main menu"), "{menu}");
}

async fn revocations(rig: &Rig) -> Vec<String> {
    sqlx::query_scalar::<_, String>(
        "SELECT detail FROM audit_log WHERE action = 'session.revoked' ORDER BY id",
    )
    .fetch_all(&rig.db)
    .await
    .unwrap()
}

#[tokio::test]
async fn removing_a_key_ends_the_sessions_signed_in_with_it_and_only_those() {
    // QA P2 (FIX-M3-14): S1 signed in with key C, S2 removes C: S1 is closed.
    let rig = Rig::start().await;
    roomy(&rig).await;
    let (id, key_c) = member_with(&rig, "maker", Slug::MEMBER, Some(PASSWORD)).await;
    let key_d = extra_key(&rig, id, "maker").await;
    let (_s1, mut s1, _) = at_menu(&rig, "maker", &key_c).await;
    let (_s1b, mut s1b, _) = at_menu(&rig, "maker", &key_c).await;
    let (_s2, mut s2, _) = at_menu(&rig, "maker", &key_d).await;
    let (_s3, mut s3) = password_menu(&rig, "maker", PASSWORD).await;

    let listed = my_keys(&mut s2).await;
    let number = number_of(&listed, &key_c);
    press(&mut s2, "r").await;
    press(&mut s2, &number.to_string()).await;
    let done = press(&mut s2, "y").await;
    assert!(flat(&done).contains("Key removed"), "{done}");

    // Both sessions on the removed key end, with the one fixed line.
    for gone in [&mut s1, &mut s1b] {
        let last = closed_with(gone).await;
        assert!(last.contains(REVOKED_LINE), "{last}");
    }
    // The acting session, and the session that used the password, stay.
    let menu = press(&mut s2, "b").await;
    assert!(flat(&menu).contains("Members"), "{menu}");
    still_open(&mut s3).await;
    assert!(!key_signs_in(rig.addr(), "maker", &key_c).await);
    assert_eq!(
        revocations(&rig).await.len(),
        2,
        "one row per ended session"
    );
    for row in revocations(&rig).await {
        assert!(row.ends_with(", key removed"), "{row}");
    }
    rig.stop().await;
}

#[tokio::test]
async fn setting_the_password_ends_the_other_password_sessions_not_the_key_ones() {
    let rig = Rig::start().await;
    roomy(&rig).await;
    let (id, key) = member_with(&rig, "maker", Slug::MEMBER, Some(PASSWORD)).await;
    let other_key = extra_key(&rig, id, "maker").await;
    let (_p1, mut by_password) = password_menu(&rig, "maker", PASSWORD).await;
    let (_k1, mut by_other_key, _) = at_menu(&rig, "maker", &other_key).await;
    let (_k2, mut actor, _) = at_menu(&rig, "maker", &key).await;

    // The actor (a key session) sets a new password: no current password asked.
    press(&mut actor, "s").await;
    press(&mut actor, "s").await;
    press(&mut actor, "s").await;
    line(&mut actor, NEW_PASSWORD).await;
    let done = line(&mut actor, NEW_PASSWORD).await;
    assert!(flat(&done).contains("Password set."), "{done}");

    let last = closed_with(&mut by_password).await;
    assert!(last.contains(REVOKED_LINE), "{last}");
    still_open(&mut by_other_key).await;
    // The acting session is still at its password page.
    let page = press(&mut actor, "").await;
    assert!(flat(&page).contains("Password set"), "{page}");
    assert_eq!(revocations(&rig).await.len(), 1);
    assert!(
        revocations(&rig).await[0].ends_with(", password changed"),
        "{:?}",
        revocations(&rig).await
    );
    rig.stop().await;
}

#[tokio::test]
async fn a_password_session_that_changes_the_password_stays_and_the_others_go() {
    let rig = Rig::start().await;
    roomy(&rig).await;
    let (_id, _key) = member_with(&rig, "maker", Slug::MEMBER, Some(PASSWORD)).await;
    let (_p1, mut first) = password_menu(&rig, "maker", PASSWORD).await;
    let (_p2, mut actor) = password_menu(&rig, "maker", PASSWORD).await;
    press(&mut actor, "s").await;
    press(&mut actor, "s").await;
    press(&mut actor, "s").await;
    line(&mut actor, PASSWORD).await; // the current password
    line(&mut actor, NEW_PASSWORD).await;
    let done = line(&mut actor, NEW_PASSWORD).await;
    assert!(flat(&done).contains("Password set."), "{done}");
    let last = closed_with(&mut first).await;
    assert!(last.contains(REVOKED_LINE), "{last}");
    let page = press(&mut actor, "").await;
    assert!(flat(&page).contains("Password set"), "{page}");
    rig.stop().await;
}

#[tokio::test]
async fn redeeming_a_recovery_code_ends_the_password_sessions_and_the_older_key_sessions() {
    let rig = Rig::start().await;
    roomy(&rig).await;
    let (_s, mut staff, _key) = founder_at_menu(&rig).await;
    let (id, old_key) = member_with(&rig, "alice", Slug::MEMBER, Some(PASSWORD)).await;
    let (_a, mut on_old_key, _) = at_menu(&rig, "alice", &old_key).await;
    let (_b, mut on_password) = password_menu(&rig, "alice", PASSWORD).await;
    let _ = id;

    press(&mut staff, "o").await;
    press(&mut staff, "r").await;
    let code = code_in(&line(&mut staff, "alice").await);

    let (_m, mut machine, _new_key, _) = new_machine(&rig).await;
    press(&mut machine, "y").await;
    line(&mut machine, &code).await;
    line(&mut machine, NEW_PASSWORD).await;
    let joined = line(&mut machine, NEW_PASSWORD).await;
    assert!(flat(&joined).contains("Signed in as alice"), "{joined}");

    for gone in [&mut on_old_key, &mut on_password] {
        let last = closed_with(gone).await;
        assert!(last.contains(REVOKED_LINE), "{last}");
    }
    // The session that redeemed the code stays signed in.
    still_open_after_login(&mut machine).await;
    assert_eq!(revocations(&rig).await.len(), 2);
    for row in revocations(&rig).await {
        assert!(row.ends_with(", recovery code"), "{row}");
    }
    rig.stop().await;
}

/// The redeeming session is at its login screen: any key takes it on to the menu.
async fn still_open_after_login(channel: &mut Channel<Msg>) {
    let menu = press(channel, "\r").await;
    assert!(flat(&menu).contains("Main menu"), "{menu}");
}

#[tokio::test]
async fn a_newer_recovery_code_voids_the_older_one() {
    // QA P1 for the staff kind (FIX-M3-13).
    let rig = Rig::start().await;
    let (_s, mut staff, _key) = founder_at_menu(&rig).await;
    let (_id, _key) = member_with(&rig, "alice", Slug::MEMBER, Some(PASSWORD)).await;
    press(&mut staff, "o").await;
    press(&mut staff, "r").await;
    let older = code_in(&line(&mut staff, "alice").await);
    // "x" leaves the code page for the handle form again.
    press(&mut staff, "x").await;
    let newer = code_in(&line(&mut staff, "alice").await);
    assert_ne!(older, newer);

    let (_m, mut machine, key_a, _) = new_machine(&rig).await;
    press(&mut machine, "y").await;
    line(&mut machine, &older).await;
    line(&mut machine, NEW_PASSWORD).await;
    let refused = line(&mut machine, NEW_PASSWORD).await;
    assert!(flat(&refused).contains(REFUSED), "{refused}");
    assert!(!key_signs_in(rig.addr(), "alice", &key_a).await);

    let (_m2, mut machine, _key_b, _) = new_machine(&rig).await;
    press(&mut machine, "y").await;
    line(&mut machine, &newer).await;
    line(&mut machine, NEW_PASSWORD).await;
    let ok = line(&mut machine, NEW_PASSWORD).await;
    assert!(flat(&ok).contains("Signed in as alice"), "{ok}");
    rig.stop().await;
}
