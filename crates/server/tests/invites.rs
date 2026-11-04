//! Invites over a real SSH client (M2-D): a member creates a code on the 80x24 screen, a
//! stranger redeems it in the waiting room and becomes a member, every failed redemption reads
//! the same, the quota stops creation, and the tree is gated by `invites.view_tree`. Everything
//! uses a temporary database and keys generated here.
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
use russh::Channel;
use russh::ChannelMsg;
use russh::client::{AuthResult, Handle, Msg};
use russh::keys::{HashAlg, PrivateKey, PrivateKeyWithHashAlg};
use russh_bbs_core::access::Fingerprint;
use russh_bbs_core::audit::{Action, AuditQuery, AuditStore};
use russh_bbs_core::invites::InviteStore;
use russh_bbs_core::store::{AccountState, AccountStore};
use russh_bbs_core::waiting_room::RatePolicy;
use russh_bbs_server::founder;
use russh_bbs_server::ssh::{HostKey, Limits, ServerConfig, SshServer};
use russh_bbs_store::SqliteStore;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};
use tempfile::TempDir;
use tokio::time::timeout;

/// What a stranger reads when a redemption does not work.
const REFUSED: &str = "That invite could not be used";

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

/// Make a code as the signed-in member at `channel`, returning the code and the screen.
async fn create_code(channel: &mut Channel<Msg>) -> (String, String) {
    press(channel, "i").await;
    let screen = press(channel, "c").await;
    let text = flat(&screen);
    let Some(rest) = text.split("Invite code: ").nth(1) else {
        panic!("no code on screen: {screen}");
    };
    let code: String = rest.chars().take(39).collect();
    assert_eq!(code.len(), 39, "{code}");
    (code, screen)
}

/// Redeem `code` as `handle` from a fresh stranger key; returns the screen after the last Enter
/// and the key.
async fn redeem_as_stranger(
    rig: &Rig,
    code: &str,
    handle: &str,
) -> (Handle<Client>, Channel<Msg>, PrivateKey, String) {
    let (session, mut channel, key, _) = stranger(rig).await;
    press(&mut channel, "c").await;
    line(&mut channel, code).await;
    let screen = line(&mut channel, handle).await;
    (session, channel, key, screen)
}

fn fingerprint_of(key: &PrivateKey) -> Fingerprint {
    Fingerprint::parse(&key.public_key().fingerprint(HashAlg::Sha256).to_string()).unwrap()
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // one walkthrough, start to finish
async fn a_member_invites_a_stranger_who_redeems_and_becomes_a_member() {
    let rig = Rig::start().await;
    let (_founder_session, mut founder, _) = founder_at_menu(&rig).await;

    // The create screen, at 80x24: the code, when it expires, that it is shown once, and the
    // reserved status row underneath.
    let (code, screen) = create_code(&mut founder).await;
    let words = flat(&screen);
    assert!(words.contains("Invite someone"), "{screen}");
    assert!(
        words.contains("It expires 20") && words.contains("UTC"),
        "{screen}"
    );
    assert!(words.contains("only time it is shown"), "{screen}");
    assert!(
        words.contains("no monthly limit"),
        "the sysop is unlimited: {screen}"
    );
    assert!(
        screen.lines().count() <= 24,
        "{} rows",
        screen.lines().count()
    );
    assert!(screen.lines().all(|l| l.chars().count() <= 80), "{screen}");
    assert!(
        !screen.contains("SERVER-READABLE"),
        "no status row (D93): {screen}"
    );
    assert!(!words.to_lowercase().contains("private"));

    // Only the hash is in the database; the audit log names the invite by id.
    let hashes: Vec<String> = sqlx::query_scalar("SELECT code_hash FROM invite")
        .fetch_all(&rig.db)
        .await
        .unwrap();
    assert_eq!(hashes.len(), 1);
    assert!(!hashes[0].contains(&code.replace('-', "")));

    // Any key closes the code; it is not shown again, and the list shows it open.
    let back = press(&mut founder, "x").await;
    assert!(!flat(&back).contains(&code), "{back}");
    let list = flat(&press(&mut founder, "y").await);
    assert!(
        list.contains("Your invites") && list.contains("open until"),
        "{list}"
    );
    assert!(!list.contains(&code));

    // A stranger redeems it with a handle of their choosing and lands in a member session.
    let (_s, mut newcomer, key, screen) = redeem_as_stranger(&rig, &code, "newcomer").await;
    let words = flat(&screen);
    assert!(words.contains("Signed in as newcomer"), "{screen}");
    let account = rig
        .store
        .account_by_fingerprint(&fingerprint_of(&key))
        .await
        .unwrap()
        .expect("the key is bound to the new account");
    assert_eq!(account.state, AccountState::Active);
    assert_eq!(account.username.as_str(), "newcomer");
    let tree = rig.store.invite_tree(10).await.unwrap();
    let founder_id = tree
        .iter()
        .find(|n| n.handle.as_str() == "founder")
        .unwrap()
        .account;
    let node = tree.iter().find(|n| n.account == account.id).unwrap();
    assert_eq!(node.invited_by, Some(founder_id));
    let log =
        russh_bbs_core::audit::known_events(rig.store.page(&AuditQuery::default()).await.unwrap());
    assert!(log.iter().any(|r| r.event.action == Action::InviteCreated));
    assert!(log.iter().any(|r| r.event.action == Action::InviteRedeemed));

    // The new member goes on to the menu (login, icon check) and can invite, but the tree is
    // for `invites.view_tree` (trusted and up).
    press(&mut newcomer, "x").await;
    press(&mut newcomer, "n").await;
    let menu = flat(&press(&mut newcomer, "y").await);
    assert!(menu.contains("[i] Invites"), "{menu}");
    let submenu = flat(&press(&mut newcomer, "i").await);
    assert!(
        submenu.contains("[c] Create an invite") && submenu.contains("[y] Your invites"),
        "{submenu}"
    );
    assert!(
        !submenu.contains("Invite tree"),
        "a member does not see the tree: {submenu}"
    );
    let nothing = flat(&press(&mut newcomer, "t").await);
    assert!(!nothing.contains("who invited whom"), "{nothing}");

    // The sysop sees the tree, indented by who invited whom.
    press(&mut founder, "\x1b").await;
    let submenu = flat(&press(&mut founder, "i").await);
    assert!(submenu.contains("[t] Invite tree"), "{submenu}");
    let tree = press(&mut founder, "t").await;
    let words = flat(&tree);
    assert!(words.contains("Who invited whom: 2 people."), "{tree}");
    let rows: Vec<&str> = tree.lines().collect();
    let at = |name: &str| {
        rows.iter()
            .find(|r| r.contains(name))
            .unwrap()
            .find(name)
            .unwrap()
    };
    assert_eq!(
        at("newcomer"),
        at("founder") + 2,
        "the invitee is indented under the inviter: {tree}"
    );
    let closed = flat(&press(&mut founder, "x").await);
    assert!(!closed.contains("Who invited whom"));
    let listing = flat(&press(&mut founder, "y").await);
    assert!(listing.contains("used by newcomer"), "{listing}");

    rig.stop().await;
}

#[tokio::test]
async fn every_failed_redemption_reads_the_same_and_a_refusal_keeps_the_code() {
    let rig = Rig::start().await;
    let (_fs, mut founder, _) = founder_at_menu(&rig).await;
    let (good, _) = create_code(&mut founder).await;
    let (used, _) = {
        press(&mut founder, "x").await;
        create_code(&mut founder).await
    };
    let (expired, _) = {
        press(&mut founder, "x").await;
        create_code(&mut founder).await
    };

    // Burn `used`, and age `expired` past its expiry by editing the row.
    let (_s, _c, _k, joined) = redeem_as_stranger(&rig, &used, "first_user").await;
    assert!(
        flat(&joined).contains("Signed in as first_user"),
        "{joined}"
    );
    let hash = russh_bbs_core::invites::CodeHash::of(&expired).to_hex();
    rig.sql(&format!(
        "UPDATE invite SET created_at = 1, expires_at = 2 WHERE code_hash = '{hash}'"
    ))
    .await;

    let mut replies = Vec::new();
    for (what, code, handle) in [
        (
            "unknown code",
            "ABCD-EFGH-JKMN-PQRS-TVWX-YZ01-2345-6789",
            "fresh_one",
        ),
        (
            "unknown code, a member's handle",
            "ABCD-EFGH-JKMN-PQRS-TVWX-YZ01-2345-6789",
            "first_user",
        ),
        ("used code", used.as_str(), "fresh_two"),
        ("expired code", expired.as_str(), "fresh_three"),
        ("good code, a taken handle", good.as_str(), "first_user"),
        (
            "good code, a lookalike of a taken handle",
            good.as_str(),
            "f1rst_user",
        ),
    ] {
        let (_s, _c, key, screen) = redeem_as_stranger(&rig, code, handle).await;
        let words = flat(&screen);
        assert!(words.contains(REFUSED), "{what}: {screen}");
        assert!(
            rig.store
                .account_by_fingerprint(&fingerprint_of(&key))
                .await
                .unwrap()
                .is_none(),
            "{what}: an account was made"
        );
        replies.push((what, words));
    }
    let first = &replies[0].1;
    for (what, words) in &replies {
        assert_eq!(
            words, first,
            "{what}: the reply differs from the unknown-code reply"
        );
    }

    // The two refusals that had a good code did not consume it.
    let (_s, _c, _k, joined) = redeem_as_stranger(&rig, &good, "last_one").await;
    assert!(flat(&joined).contains("Signed in as last_one"), "{joined}");
    rig.stop().await;
}

#[tokio::test]
async fn redeeming_needs_invite_redeem_on_every_attempt() {
    let rig = Rig::start().await;
    let (_fs, mut founder, _) = founder_at_menu(&rig).await;
    let (code, _) = create_code(&mut founder).await;

    // Take the permission from the waiting role: the same valid code is refused with the same
    // words, nothing is written, and the code is not used up.
    rig.sql("DELETE FROM role_permission WHERE role = 'waiting' AND permission = 'invite.redeem'")
        .await;
    let (_s, _c, key, screen) = redeem_as_stranger(&rig, &code, "newcomer").await;
    assert!(flat(&screen).contains(REFUSED), "{screen}");
    assert!(
        rig.store
            .account_by_fingerprint(&fingerprint_of(&key))
            .await
            .unwrap()
            .is_none()
    );
    let used: i64 = sqlx::query_scalar("SELECT count(*) FROM invite WHERE redeemed_at IS NOT NULL")
        .fetch_one(&rig.db)
        .await
        .unwrap();
    assert_eq!(used, 0);

    // Put back: it applies on the next attempt.
    rig.sql("INSERT INTO role_permission (role, permission) VALUES ('waiting', 'invite.redeem')")
        .await;
    let (_s, _c, _k, screen) = redeem_as_stranger(&rig, &code, "newcomer").await;
    assert!(flat(&screen).contains("Signed in as newcomer"), "{screen}");
    rig.stop().await;
}

#[tokio::test]
async fn a_member_stops_at_their_quota_with_a_plain_message() {
    let rig = Rig::start().await;
    let (_fs, mut founder, _) = founder_at_menu(&rig).await;
    let (code, _) = create_code(&mut founder).await;
    let (_s, mut member, _k, _) = redeem_as_stranger(&rig, &code, "newcomer").await;
    press(&mut member, "x").await;
    press(&mut member, "n").await;
    press(&mut member, "y").await;
    press(&mut member, "i").await;

    // A member may make 2 a month (M2-H's default for `member`): the screens count down, then
    // say no.
    for left in [1, 0] {
        let screen = flat(&press(&mut member, "c").await);
        assert!(
            screen.contains(&format!("still make this month: {left}.")),
            "{screen}"
        );
        press(&mut member, "x").await;
    }
    let screen = flat(&press(&mut member, "c").await);
    assert!(
        screen.contains("all 2 of your invites for this month"),
        "{screen}"
    );
    assert!(screen.contains("The count restarts"), "{screen}");
    assert!(screen.contains("No invite was made."), "{screen}");
    assert!(!screen.contains("Invite code:"), "{screen}");
    let made: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM invite WHERE inviter_id = (SELECT id FROM accounts WHERE username = 'newcomer')",
    )
    .fetch_one(&rig.db)
    .await
    .unwrap();
    assert_eq!(made, 2);
    rig.stop().await;
}

#[tokio::test]
async fn the_tree_follows_its_permission_on_the_next_action() {
    let rig = Rig::start().await;
    let (_fs, mut founder, _) = founder_at_menu(&rig).await;
    press(&mut founder, "i").await;
    assert!(flat(&press(&mut founder, "x").await).contains("[t] Invite tree"));

    // Take `invites.view_tree` from every role that holds it (the founder is sysop and member).
    rig.sql("DELETE FROM role_permission WHERE permission = 'invites.view_tree'")
        .await;
    let after = flat(&press(&mut founder, "t").await);
    assert!(!after.contains("Who invited whom"), "{after}");
    assert!(!after.contains("[t] Invite tree"), "{after}");
    rig.sql("INSERT INTO role_permission (role, permission) VALUES ('sysop', 'invites.view_tree')")
        .await;
    assert!(flat(&press(&mut founder, "x").await).contains("[t] Invite tree"));
    let tree = flat(&press(&mut founder, "t").await);
    assert!(tree.contains("Who invited whom: 1 people."), "{tree}");
    rig.stop().await;
}
