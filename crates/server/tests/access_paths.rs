//! Every front-end action path goes through `core::access` (M2-B, D33).
//!
//! Compile time: core action functions take a `Granted` that only `Snapshot::require` makes, so
//! a path cannot skip the check. This file is the behavioural half, over a real russh client
//! against the real SQLite store: each wired permission is removed from the role that gives it
//! (raw SQL, because the editing services are a later slice) and the path must refuse and write
//! nothing; put back, it works again; and a removal applies on the very next action of an open
//! session. The two lists below make a later slice move its permissions across: a permission
//! that is in neither list, or in both, fails `every_permission_is_wired_or_listed`.
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
use russh::keys::{HashAlg, PrivateKey, PrivateKeyWithHashAlg};
use russh::{Channel, ChannelMsg};
use russh_bbs_core::access::{Fingerprint, Permission};
use russh_bbs_core::store::AccountStore;
use russh_bbs_core::terminal_settings::{TerminalSettings, TerminalSettingsStore};
use russh_bbs_server::founder;
use russh_bbs_server::ssh::{HostKey, Limits, ServerConfig, SshServer};
use russh_bbs_store::SqliteStore;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};
use tempfile::TempDir;
use tokio::time::timeout;

/// Permissions whose path is exercised below. Menu items: the hotkey, and the label the screen
/// shows while the permission is held.
///
/// `InviteRedeem` and `InvitesViewTree` have their path tests in `tests/invites.rs` (the
/// redeem form refuses without the permission on every attempt; the tree item and screen follow
/// `invites.view_tree` on the next action); `InvitesCreate` is in the menu-path test below.
const WIRED: &[Permission] = &[
    Permission::WaitingView,
    Permission::InviteRedeem,
    Permission::InvitesCreate,
    Permission::InvitesViewTree,
    Permission::AccountEditSettings,
    Permission::MembersView,
    Permission::MembersApprove,
    Permission::ForumList,
    Permission::MailRead,
    Permission::FilesList,
];

/// Permissions with no front-end action yet. A slice that builds one moves it to `WIRED` in the
/// same PR, with a path test, and adds its row to `docs/access.md`.
const NOT_YET_WIRED: &[Permission] = &[
    Permission::MembershipApply,
    Permission::AccountEditProfile,
    Permission::AccountSetPassword,
    Permission::MembersDeny,
    Permission::MembersVouch,
    Permission::MembersSuspend,
    Permission::MembersEdit,
    Permission::MembersDeleteData,
    Permission::InvitesRevoke,
    Permission::ForumRead,
    Permission::ForumPost,
    Permission::ForumReply,
    Permission::ForumEditOwn,
    Permission::ForumModerate,
    Permission::ForumManage,
    Permission::ChatJoin,
    Permission::ChatSpeak,
    Permission::ChatWhisper,
    Permission::ChatModerate,
    Permission::ChatManageRooms,
    Permission::MailSend,
    Permission::FilesDownload,
    Permission::FilesUpload,
    Permission::FilesTag,
    Permission::FilesApprove,
    Permission::FilesManageAreas,
    Permission::FeedsRead,
    Permission::FeedsManage,
    Permission::DoorsRun,
    Permission::DoorsManage,
    Permission::OnelinersPost,
    Permission::GraffitiPost,
    Permission::PollsVote,
    Permission::PollsCreate,
    Permission::EventsView,
    Permission::EventsManage,
    Permission::SysopPage,
    Permission::SysopChat,
    Permission::SessionsView,
    Permission::SessionsDisconnect,
    Permission::AuditView,
    Permission::SettingsEdit,
    Permission::RolesEdit,
    Permission::GroupsEdit,
    Permission::ThemesManage,
    Permission::BackupsRun,
];

#[test]
fn every_permission_is_wired_or_listed() {
    for &permission in Permission::ALL {
        let wired = WIRED.contains(&permission);
        let listed = NOT_YET_WIRED.contains(&permission);
        assert!(
            wired != listed,
            "{} must be in exactly one of WIRED and NOT_YET_WIRED",
            permission.name()
        );
    }
    assert_eq!(WIRED.len() + NOT_YET_WIRED.len(), Permission::ALL.len());
}

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
            store,
            db,
            token,
            _dir: dir,
        }
    }

    fn addr(&self) -> SocketAddr {
        self.server.local_addr()
    }

    /// Take `permission` away from every role the founder holds (`sysop`, and `member` through
    /// `everyone`): access is additive, so removing it from only one would change nothing.
    /// Stands in for a role edit, which is a later slice. Applies on the next action. Returns
    /// the roles it was taken from, for `restore`.
    async fn revoke(&self, permission: Permission) -> Vec<String> {
        let roles: Vec<String> = sqlx::query_scalar(
            "SELECT role FROM role_permission WHERE permission = ? AND role IN ('sysop', 'member')",
        )
        .bind(permission.name())
        .fetch_all(&self.db)
        .await
        .unwrap();
        assert!(
            !roles.is_empty(),
            "{} was held by no role",
            permission.name()
        );
        for role in &roles {
            sqlx::query("DELETE FROM role_permission WHERE role = ? AND permission = ?")
                .bind(role)
                .bind(permission.name())
                .execute(&self.db)
                .await
                .unwrap();
        }
        roles
    }

    async fn restore(&self, roles: &[String], permission: Permission) {
        for role in roles {
            sqlx::query("INSERT INTO role_permission (role, permission) VALUES (?, ?)")
                .bind(role)
                .bind(permission.name())
                .execute(&self.db)
                .await
                .unwrap();
        }
    }

    async fn stored(&self, key: &PrivateKey) -> TerminalSettings {
        let fp =
            Fingerprint::parse(&key.public_key().fingerprint(HashAlg::Sha256).to_string()).unwrap();
        let account = self
            .store
            .account_by_fingerprint(&fp)
            .await
            .unwrap()
            .unwrap();
        self.store.terminal_settings(account.id).await.unwrap()
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

/// Read until the channel is quiet; returns the printable text as words.
async fn words(channel: &mut Channel<client::Msg>) -> String {
    let mut raw = String::new();
    while let Ok(msg) = timeout(Duration::from_millis(200), channel.wait()).await {
        match msg {
            Some(ChannelMsg::Data { data }) => raw.push_str(&String::from_utf8_lossy(&data)),
            Some(ChannelMsg::Close | ChannelMsg::Eof) | None => break,
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
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

async fn send(channel: &mut Channel<client::Msg>, bytes: &str) -> String {
    channel.data_bytes(bytes.as_bytes().to_vec()).await.unwrap();
    words(channel).await
}

/// The whole current screen (a window change makes the server clear and redraw everything).
async fn full(channel: &mut Channel<client::Msg>) -> String {
    channel.window_change(80, 24, 0, 0).await.unwrap();
    words(channel).await
}

/// Claim the board as the founder and answer the icon check; returns the channel at the menu.
async fn founder_at_menu(rig: &Rig, key: &PrivateKey) -> (Handle<Client>, Channel<client::Msg>) {
    let session = login(rig.addr(), "claim", key).await;
    let mut channel = common::open_shell(&session, 80, 24).await;
    words(&mut channel).await;
    send(&mut channel, &format!("{}\r", rig.token)).await;
    send(&mut channel, "x").await;
    send(&mut channel, "n").await;
    send(&mut channel, "y").await;
    (session, channel)
}

#[tokio::test]
async fn menu_paths_follow_their_permission_on_the_next_action() {
    let rig = Rig::start().await;
    let key = new_client_key();
    let (_session, mut channel) = founder_at_menu(&rig, &key).await;
    // (permission, hotkey, label shown while held, notice a pick would show)
    let paths = [
        (
            Permission::ForumList,
            'f',
            "[f] Forums",
            "Forums are not built yet.",
        ),
        (
            Permission::MailRead,
            'm',
            "[m] Mail",
            "Mail is not built yet.",
        ),
        (
            Permission::FilesList,
            'a',
            "[a] Files",
            "File areas are not built yet.",
        ),
        (
            Permission::MembersView,
            'u',
            "[u] Members",
            "The members list is not built yet.",
        ),
        (
            Permission::InvitesCreate,
            'i',
            "[i] Invites",
            "Create an invite",
        ),
        (
            Permission::MembersApprove,
            'o',
            "[o] Sysop tools",
            "Sysop tools",
        ),
    ];
    for (permission, hotkey, label, effect) in paths {
        let menu = full(&mut channel).await;
        assert!(menu.contains(label), "{label} missing while held: {menu}");

        // Taken away: the very next action no longer offers the item, and its hotkey does
        // nothing (not found by name either).
        let taken = rig.revoke(permission).await;
        let after = send(&mut channel, &hotkey.to_string()).await;
        let screen = format!("{after} {}", full(&mut channel).await);
        assert!(!screen.contains(label), "{label} still offered: {screen}");
        assert!(!screen.contains(effect), "{hotkey} still worked: {screen}");

        // Put back: it applies on the next action again.
        rig.restore(&taken, permission).await;
        send(&mut channel, "x").await;
        assert!(full(&mut channel).await.contains(label), "{label} not back");
        // Leave any submenu the working pick opened.
        send(&mut channel, "\u{1b}").await;
    }
    rig.server.shutdown().await;
}

#[tokio::test]
async fn saving_settings_needs_account_edit_settings_and_a_removal_saves_nothing() {
    let rig = Rig::start().await;
    let key = new_client_key();
    let (_session, mut channel) = founder_at_menu(&rig, &key).await;
    let before = rig.stored(&key).await;

    // Held: the item is there and a change is stored.
    assert!(full(&mut channel).await.contains("[s] Terminal settings"));
    send(&mut channel, "s").await;
    assert!(full(&mut channel).await.contains("Width: automatic"));
    send(&mut channel, "c").await;
    let toggled = rig.stored(&key).await;
    assert_ne!(toggled.color(), before.color(), "the change was stored");

    // Taken away while the settings page is open: the next key leaves the page, says why, and
    // writes nothing.
    let taken = rig.revoke(Permission::AccountEditSettings).await;
    let screen = send(&mut channel, "c").await;
    let screen = format!("{screen} {}", full(&mut channel).await);
    assert!(
        screen.contains("You are not allowed to change these settings."),
        "{screen}"
    );
    assert!(screen.contains("Main menu"), "{screen}");
    assert_eq!(
        rig.stored(&key).await.color(),
        toggled.color(),
        "nothing written"
    );

    // The item is gone and its hotkey does nothing.
    assert!(!full(&mut channel).await.contains("Terminal settings"));
    send(&mut channel, "s").await;
    assert!(full(&mut channel).await.contains("Main menu"));

    // Put back: it works again on the next action.
    rig.restore(&taken, Permission::AccountEditSettings).await;
    send(&mut channel, "x").await;
    send(&mut channel, "s").await;
    assert!(full(&mut channel).await.contains("Width: automatic"));
    send(&mut channel, "c").await;
    assert_eq!(rig.stored(&key).await.color(), before.color());
    rig.server.shutdown().await;
}

#[tokio::test]
async fn the_waiting_room_needs_waiting_view_and_refuses_the_shell_without_it() {
    let rig = Rig::start().await;
    let stranger = new_client_key();

    // Held: an unknown key reaches the waiting room.
    let session = login(rig.addr(), "anyone", &stranger).await;
    let mut channel = common::open_shell(&session, 80, 24).await;
    assert!(words(&mut channel).await.contains("Waiting room"));
    drop(channel);
    drop(session);

    // Removed: the same key is refused a shell. It sees nothing, not a looser screen.
    sqlx::query("DELETE FROM role_permission WHERE role = 'waiting' AND permission = ?")
        .bind(Permission::WaitingView.name())
        .execute(&rig.db)
        .await
        .unwrap();
    let session = login(rig.addr(), "anyone", &stranger).await;
    let mut channel = session.channel_open_session().await.unwrap();
    channel
        .request_pty(true, "xterm-256color", 80, 24, 0, 0, &[])
        .await
        .unwrap();
    common::expect_reply(&mut channel, true).await;
    channel.request_shell(true).await.unwrap();
    common::expect_reply(&mut channel, false).await;
    drop(channel);
    drop(session);

    // Put back: it works again.
    sqlx::query("INSERT INTO role_permission (role, permission) VALUES ('waiting', ?)")
        .bind(Permission::WaitingView.name())
        .execute(&rig.db)
        .await
        .unwrap();
    let session = login(rig.addr(), "anyone", &stranger).await;
    let mut channel = common::open_shell(&session, 80, 24).await;
    assert!(words(&mut channel).await.contains("Waiting room"));
    rig.server.shutdown().await;
}
