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
use russh_bbs_core::access::AccessStore;
use russh_bbs_core::access::{Fingerprint, Permission, Slug};
use russh_bbs_core::limits::{
    Limit, LimitEditError, LimitKind, LimitStore, clear_member_limit, set_member_limit,
    set_role_limit,
};
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
///
/// `InviteRequest`, `MembersDeny` and `MembersVouch` (M2-E) have their path tests in
/// `tests/queue.rs`: without `invite.request` the request form gives the same answer and stores
/// nothing, and without `members.deny` or `members.vouch` the key is not offered, pressing it
/// does nothing and nothing is written, on the very next action. `MembersApprove` is also
/// exercised there (the screen it opens and the approve key), as well as in the menu-path test
/// below.
///
/// `ChatJoin`, `ChatSpeak`, `ChatWhisper` and `ChatManageRooms` (M3-F) have their path tests in
/// `tests/chat.rs`: with `chat.speak` or `chat.whisper` taken a line or whisper is refused and
/// nobody hears it; with `chat.join` taken the member is put out of the room on the next action and
/// the menu stops offering Chat; room management keys do nothing without `chat.manage_rooms`; each
/// is back on the next action once restored. `ChatModerate` is not built yet.
///
/// `MembershipApply` and `MembershipOpen` (M2-G) have their path tests in
/// `tests/registration.rs`: with `membership.apply` taken from the `waiting` role the application
/// form gives the same words and stores nothing, and with `membership.open` taken the
/// create-account form gives its one refusal and makes no account, each on the very next
/// attempt.
///
/// The keys permissions (FIX-M3-11) have their path tests at the end of this file: without
/// `keys.manage` My keys is not offered and a screen already open closes on the next key; without
/// `keys.redeem` the waiting room refuses a valid code with the one sentence and the code is not
/// used up; without `keys.recover` the staff screen is not offered. Each is back on the next action.
///
/// `doors.run` (M4-D0) has its path test at the end of this file: without it the Doors menu item
/// is not offered, an open door closes on the next key and writes nothing more, and it is back
/// on the next action once restored.
///
/// The community permissions (M3-J) have their path tests at the end of this file: posting a
/// one-liner or a wall line, voting, making a poll, reading, and moderating each refuse and write
/// nothing once the permission is taken away, on the very next key.
const WIRED: &[Permission] = &[
    Permission::WaitingView,
    Permission::InviteRequest,
    Permission::MembershipApply,
    Permission::MembershipOpen,
    Permission::InviteRedeem,
    Permission::InvitesCreate,
    Permission::InvitesViewTree,
    Permission::AccountEditSettings,
    Permission::MembersView,
    Permission::MembersApprove,
    Permission::MembersDeny,
    Permission::MembersVouch,
    Permission::AccountEditProfile,
    Permission::MembersEdit,
    Permission::ForumList,
    Permission::ForumRead,
    Permission::ForumPost,
    Permission::ForumReply,
    Permission::ForumModerate,
    Permission::ForumManage,
    Permission::MailRead,
    Permission::MailSend,
    Permission::MailReport,
    Permission::KeysManage,
    Permission::KeysRedeem,
    Permission::KeysRecover,
    Permission::FilesList,
    Permission::LimitsEdit,
    Permission::ChatJoin,
    Permission::ChatSpeak,
    Permission::ChatWhisper,
    Permission::ChatManageRooms,
    Permission::OnelinersPost,
    Permission::GraffitiPost,
    Permission::PollsVote,
    Permission::PollsCreate,
    Permission::CommunityView,
    Permission::CommunityModerate,
    Permission::DoorsRun,
];

/// Permissions with no front-end action yet. A slice that builds one moves it to `WIRED` in the
/// same PR, with a path test, and adds its row to `docs/access.md`.
const NOT_YET_WIRED: &[Permission] = &[
    Permission::AccountSetPassword,
    Permission::MembersSuspend,
    Permission::MembersDeleteData,
    Permission::InvitesRevoke,
    Permission::ForumEditOwn,
    Permission::ChatModerate,
    // Lists and closes reports only; its screen is the M5 sysop console's (D90). Tested in the
    // shared store suite.
    Permission::MailModerate,
    Permission::FilesDownload,
    Permission::FilesUpload,
    Permission::FilesTag,
    Permission::FilesApprove,
    Permission::FilesManageAreas,
    Permission::FeedsRead,
    Permission::FeedsManage,
    // The per-door staff tools (M4-D0, D98) have core functions, tested against the real role data
    // in the shared store suite (`crates/store/src/suite/doors.rs`), and no screen yet.
    Permission::DoorsManage,
    Permission::DoorsResetPlayer,
    Permission::DoorsEndSeason,
    Permission::DoorsBarPlayer,
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
        let keys = russh_bbs_core::mail::MailKeys::single(
            russh_bbs_core::mail::KeyId::FIRST,
            russh_bbs_core::mail::MailKey::generate().unwrap(),
        );
        let ui = russh_bbs_server::ui::Ui::standard()
            .unwrap()
            .with_mail_keys(Arc::new(keys))
            .with_doors(russh_bbs_server::doors::Doors::builtin().unwrap());
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

    /// Another active member, by name, for the profile paths.
    async fn other_member(&self, name: &str) -> russh_bbs_core::store::AccountId {
        self.store
            .create_account(russh_bbs_core::store::NewAccount {
                username: russh_bbs_core::sanitize::sanitize_username(name).unwrap(),
                role: russh_bbs_core::access::Slug::MEMBER,
                state: russh_bbs_core::store::AccountState::Active,
            })
            .await
            .unwrap()
            .id
    }

    async fn profile_of(
        &self,
        id: russh_bbs_core::store::AccountId,
    ) -> russh_bbs_core::profiles::Profile {
        use russh_bbs_core::profiles::ProfileStore;
        self.store.member(id).await.unwrap().unwrap().profile
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
            &common::user_for(key, user),
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

/// Redraw until the screen shows `needle`, bounded by `common::WAIT`; panics with the last
/// screen seen. Use it instead of sleeping before a read.
async fn full_until(channel: &mut Channel<client::Msg>, needle: &str) -> String {
    let mut last = String::new();
    let reached = timeout(common::WAIT, async {
        loop {
            last = full(channel).await;
            if last.contains(needle) {
                return;
            }
        }
    })
    .await;
    assert!(
        reached.is_ok(),
        "no {needle:?} in time; last screen: {last}"
    );
    last
}

/// Claim the board as the founder and answer the icon check; returns the channel at the menu.
async fn founder_at_menu(rig: &Rig, key: &PrivateKey) -> (Handle<Client>, Channel<client::Msg>) {
    let session = login(rig.addr(), "claim", key).await;
    let mut channel = common::open_shell(&session, 80, 24).await;
    words(&mut channel).await;
    send(&mut channel, &format!("{}\r", rig.token)).await;
    send(&mut channel, &format!("{}\r", "founder password one")).await;
    send(&mut channel, &format!("{}\r", "founder password one")).await;
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
            "There are no boards you can open yet.",
        ),
        (
            Permission::MailRead,
            'm',
            "[m] Mail",
            "Your mailbox is empty.",
        ),
        (
            Permission::FilesList,
            'a',
            "[a] Files",
            "File areas are not built yet.",
        ),
        (Permission::MembersView, 'u', "[u] Members", "Member list"),
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

/// `limits.edit` has no front-end screen yet (the sysop console is M5); its path is the core
/// editing service. The proof comes from the account's own snapshot, so removing the permission
/// from the roles refuses the next edit and nothing is written; putting it back works again.
#[tokio::test]
async fn editing_limits_needs_limits_edit_and_a_removal_writes_nothing() {
    let rig = Rig::start().await;
    let key = new_client_key();
    let (_session, _channel) = founder_at_menu(&rig, &key).await;
    let fp =
        Fingerprint::parse(&key.public_key().fingerprint(HashAlg::Sha256).to_string()).unwrap();
    let account = rig
        .store
        .account_by_fingerprint(&fp)
        .await
        .unwrap()
        .unwrap()
        .id;
    let kind = LimitKind::MinutesPerDay;
    let proof = |rig: &Rig| {
        let store = rig.store.clone();
        async move {
            store
                .snapshot(Some(account))
                .await
                .unwrap()
                .require(Permission::LimitsEdit, None)
        }
    };

    // Held: the edit works for a role and for the account.
    let granted = proof(&rig).await.unwrap();
    set_role_limit(&rig.store, &granted, &Slug::MEMBER, kind, Limit::AtMost(99))
        .await
        .unwrap();
    set_member_limit(&rig.store, &granted, account, kind, Limit::AtMost(7))
        .await
        .unwrap();
    assert_eq!(rig.store.member_limits(account).await.unwrap().len(), 1);

    // Removed: there is no proof to pass, so the service cannot be reached, and a proof for a
    // different permission is refused by the service itself.
    let taken = rig.revoke(Permission::LimitsEdit).await;
    assert!(proof(&rig).await.is_err());
    let other = rig
        .store
        .snapshot(Some(account))
        .await
        .unwrap()
        .require(Permission::AccountEditSettings, None)
        .unwrap();
    assert!(matches!(
        set_role_limit(&rig.store, &other, &Slug::MEMBER, kind, Limit::AtMost(1)).await,
        Err(LimitEditError::NotPermitted)
    ));
    assert!(matches!(
        clear_member_limit(&rig.store, &other, account, kind).await,
        Err(LimitEditError::NotPermitted)
    ));
    let role_rows = rig.store.role_limits(&[Slug::MEMBER]).await.unwrap();
    assert!(
        role_rows
            .iter()
            .any(|r| r.kind == kind && r.limit == Limit::AtMost(99)),
        "nothing was written"
    );
    assert_eq!(rig.store.member_limits(account).await.unwrap().len(), 1);

    // Put back: it works on the next action.
    rig.restore(&taken, Permission::LimitsEdit).await;
    let granted = proof(&rig).await.unwrap();
    clear_member_limit(&rig.store, &granted, account, kind)
        .await
        .unwrap();
    assert_eq!(rig.store.member_limits(account).await.unwrap().len(), 0);
    rig.server.shutdown().await;
}

#[tokio::test]
async fn editing_your_own_profile_needs_account_edit_profile_and_a_removal_saves_nothing() {
    use russh_bbs_core::profiles::Field;
    let rig = Rig::start().await;
    let key = new_client_key();
    let (_session, mut channel) = founder_at_menu(&rig, &key).await;
    let founder_id = {
        let fp =
            Fingerprint::parse(&key.public_key().fingerprint(HashAlg::Sha256).to_string()).unwrap();
        rig.store
            .account_by_fingerprint(&fp)
            .await
            .unwrap()
            .unwrap()
            .id
    };

    // Held: the item is offered and a save is stored.
    send(&mut channel, "u").await;
    assert!(full(&mut channel).await.contains("[e] Edit my profile"));
    send(&mut channel, "e").await;
    assert!(full(&mut channel).await.contains("> Display name:"));
    send(&mut channel, "Founder\r\r\r\r\r").await;
    assert_eq!(
        rig.profile_of(founder_id).await.get(Field::DisplayName),
        "Founder"
    );

    // Taken away while the form is open: the next key closes it, says why and writes nothing.
    // (Staff keep `members.edit`, which is about other people and does not stand in.)
    send(&mut channel, "e").await;
    assert!(full(&mut channel).await.contains("> Display name:"));
    let taken = rig.revoke(Permission::AccountEditProfile).await;
    let first = send(&mut channel, "!").await;
    let screen = format!(
        "{first} {} {}",
        send(&mut channel, "\r\r\r\r\r").await,
        full(&mut channel).await
    );
    assert!(
        screen.contains("You are not allowed to do that."),
        "{screen}"
    );
    assert_eq!(
        rig.profile_of(founder_id).await.get(Field::DisplayName),
        "Founder",
        "nothing written"
    );
    // The item is gone and its hotkey does nothing.
    assert!(!full(&mut channel).await.contains("Edit my profile"));
    send(&mut channel, "e").await;
    assert!(!full(&mut channel).await.contains("> Display name:"));

    // Put back: it works again on the next action.
    rig.restore(&taken, Permission::AccountEditProfile).await;
    send(&mut channel, "x").await;
    send(&mut channel, "e").await;
    assert!(full(&mut channel).await.contains("> Display name:"));
    rig.server.shutdown().await;
}

#[tokio::test]
async fn editing_another_profile_needs_members_edit_and_a_removal_saves_nothing() {
    use russh_bbs_core::profiles::Field;
    let rig = Rig::start().await;
    let bob = rig.other_member("bob").await;
    let key = new_client_key();
    let (_session, mut channel) = founder_at_menu(&rig, &key).await;

    // Held: Bob (sorts first) offers the edit key and the form says whose profile it is.
    send(&mut channel, "ul1").await;
    let view = full(&mut channel).await;
    assert!(
        view.contains("Handle: bob") && view.contains("e: edit"),
        "{view}"
    );

    // Taken away: the footer stops offering it on the very next action, and the key does nothing.
    let taken = rig.revoke(Permission::MembersEdit).await;
    send(&mut channel, "x").await;
    assert!(!full(&mut channel).await.contains("e: edit"));
    send(&mut channel, "e").await;
    assert!(!full(&mut channel).await.contains("staff edit"));
    assert_eq!(rig.profile_of(bob).await.get(Field::Location), "");

    // Put back: the form opens; then it is taken away again while open, and the save is refused.
    rig.restore(&taken, Permission::MembersEdit).await;
    send(&mut channel, "x").await;
    send(&mut channel, "e").await;
    assert!(
        full(&mut channel)
            .await
            .contains("Editing the profile of bob (staff edit)")
    );
    let taken = rig.revoke(Permission::MembersEdit).await;
    let first = send(&mut channel, "!").await;
    let screen = format!(
        "{first} {} {}",
        send(&mut channel, "\r\r\r\r\r").await,
        full(&mut channel).await
    );
    assert!(
        screen.contains("You are not allowed to do that."),
        "{screen}"
    );
    assert_eq!(
        rig.profile_of(bob).await.get(Field::DisplayName),
        "",
        "nothing written"
    );

    // Put back once more: a staff edit is stored.
    rig.restore(&taken, Permission::MembersEdit).await;
    send(&mut channel, "x").await;
    send(&mut channel, "ul1e").await;
    send(&mut channel, "Bobby\r\r\r\r\r").await;
    assert_eq!(rig.profile_of(bob).await.get(Field::DisplayName), "Bobby");
    rig.server.shutdown().await;
}

/// A member (not the founder) with a key, signed in and at the menu.
async fn member_at_menu(
    rig: &Rig,
    name: &str,
) -> (
    russh_bbs_core::store::AccountId,
    Handle<Client>,
    Channel<client::Msg>,
) {
    use russh_bbs_core::store::{AccountState, AccountStore, NewAccount};
    let key = new_client_key();
    let fp =
        Fingerprint::parse(&key.public_key().fingerprint(HashAlg::Sha256).to_string()).unwrap();
    let account = rig
        .store
        .create_account(NewAccount {
            username: russh_bbs_core::sanitize::sanitize_username(name).unwrap(),
            role: Slug::MEMBER,
            state: AccountState::Active,
        })
        .await
        .unwrap();
    rig.store.attach_fingerprint(account.id, &fp).await.unwrap();
    common::remember(fp, account.username.as_str());
    let session = login(rig.addr(), name, &key).await;
    let mut channel = common::open_shell(&session, 80, 24).await;
    words(&mut channel).await;
    // First login: any key, then the icon check (pick Unicode).
    for key in ["x", "n", "y"] {
        send(&mut channel, key).await;
    }
    (account.id, session, channel)
}

#[tokio::test]
async fn a_role_revoked_by_the_editing_service_stops_working_on_the_next_key() {
    use russh_bbs_core::access::{
        NewRole, Scope, Subject, create_role, delete_role, grant_role, revoke_role,
    };
    use russh_bbs_core::founder::FounderStore;
    let rig = Rig::start().await;
    let boss = rig
        .store
        .claim_founder(
            russh_bbs_core::sanitize::sanitize_username("boss").unwrap(),
            &Fingerprint::parse(&format!("SHA256:{:A>43}", 1)).unwrap(),
        )
        .await
        .unwrap()
        .unwrap();
    let boss_snap = rig.store.snapshot(Some(boss.id)).await.unwrap();
    let greeter = Slug::parse("greeter").unwrap();
    create_role(
        &rig.store,
        &boss_snap,
        NewRole {
            name: greeter.clone(),
            rank: 20,
            permissions: vec![Permission::MembersApprove],
        },
    )
    .await
    .unwrap();
    let (pat, _session, mut channel) = member_at_menu(&rig, "pat").await;
    let who = Subject::Account(pat);

    // Not held: a plain member has no Sysop tools.
    assert!(!full(&mut channel).await.contains("[o] Sysop tools"));

    // Granted by the service: the next action offers it and the hotkey works.
    grant_role(&rig.store, &boss_snap, who, &greeter, Scope::Global)
        .await
        .unwrap();
    send(&mut channel, "x").await;
    let menu = full(&mut channel).await;
    assert!(menu.contains("[o] Sysop tools"), "{menu}");

    // Revoked by the service: the very next key no longer offers it or does it.
    revoke_role(&rig.store, &boss_snap, who, &greeter, Scope::Global)
        .await
        .unwrap();
    let after = send(&mut channel, "o").await;
    let screen = format!("{after} {}", full(&mut channel).await);
    assert!(
        !screen.contains("[o] Sysop tools"),
        "still offered: {screen}"
    );
    assert!(
        !screen.contains("Sysop tools"),
        "the hotkey still worked: {screen}"
    );

    // With no grant left the role can go, and a later grant of it is refused (it is gone).
    delete_role(&rig.store, &boss_snap, &greeter).await.unwrap();
    assert!(
        grant_role(&rig.store, &boss_snap, who, &greeter, Scope::Global)
            .await
            .is_err()
    );
    rig.server.shutdown().await;
}

async fn count(rig: &Rig, table: &str) -> i64 {
    sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
        .fetch_one(&rig.db)
        .await
        .unwrap()
}

/// Send `bytes`, wait, and return the whole screen as words (a redraw sends only changed cells,
/// so the words of a plain `send` can be missing the unchanged letters of a message).
async fn press_full(channel: &mut Channel<client::Msg>, bytes: &str) -> String {
    send(channel, bytes).await;
    full(channel).await
}

#[tokio::test]
async fn forum_paths_follow_their_permissions_on_the_next_action() {
    const SEND: &str = "\u{18}";
    let rig = Rig::start().await;
    let key = new_client_key();
    let (_session, mut channel) = founder_at_menu(&rig, &key).await;
    let refused = "You may not do that.";

    // forum.manage: a board is made only while it is held.
    let screen = press_full(&mut channel, "f").await;
    assert!(screen.contains("n new board"), "{screen}");
    let taken = rig.revoke(Permission::ForumManage).await;
    for typed in ["n", "Alpha\r", "\r"] {
        press_full(&mut channel, typed).await;
    }
    let screen = press_full(&mut channel, "n").await;
    assert!(screen.contains(refused), "{screen}");
    assert_eq!(count(&rig, "board").await, 0, "nothing was written");
    press_full(&mut channel, "\u{1b}").await;
    rig.restore(&taken, Permission::ForumManage).await;
    for typed in ["n", "Alpha\r", "\r"] {
        press_full(&mut channel, typed).await;
    }
    let screen = press_full(&mut channel, "n").await;
    assert!(screen.contains("Board created."), "{screen}");
    assert_eq!(count(&rig, "board").await, 1);

    // forum.post: a thread is started only while it is held.
    press_full(&mut channel, "1").await;
    let taken = rig.revoke(Permission::ForumPost).await;
    press_full(&mut channel, "w").await;
    let screen = press_full(&mut channel, "First\r").await;
    assert!(screen.contains(refused), "{screen}");
    assert_eq!(count(&rig, "thread").await, 0);
    rig.restore(&taken, Permission::ForumPost).await;
    let editor = press_full(&mut channel, "\r").await;
    assert!(editor.contains("Alpha: First"), "{editor}");
    press_full(&mut channel, "opening post").await;
    let screen = press_full(&mut channel, SEND).await;
    assert!(screen.contains("Posted."), "{screen}");
    assert_eq!(count(&rig, "post").await, 1);

    // forum.read: a thread that was listed is no longer opened, and the answer is the plain
    // "not available".
    press_full(&mut channel, "\u{1b}").await;
    let taken = rig.revoke(Permission::ForumRead).await;
    let screen = press_full(&mut channel, "\r").await;
    assert!(screen.contains("That is not available."), "{screen}");
    rig.restore(&taken, Permission::ForumRead).await;
    press_full(&mut channel, "1").await;
    let screen = press_full(&mut channel, "\r").await;
    assert!(screen.contains("opening post"), "{screen}");

    // forum.reply: a reply is written only while it is held.
    let taken = rig.revoke(Permission::ForumReply).await;
    let screen = press_full(&mut channel, "r").await;
    assert!(screen.contains(refused), "{screen}");
    assert_eq!(count(&rig, "post").await, 1);
    rig.restore(&taken, Permission::ForumReply).await;
    press_full(&mut channel, "r").await;
    press_full(&mut channel, "a reply").await;
    let screen = press_full(&mut channel, SEND).await;
    assert!(screen.contains("Post 2 of 2"), "{screen}");
    assert_eq!(count(&rig, "post").await, 2);

    // forum.moderate: a post is removed only while it is held.
    let taken = rig.revoke(Permission::ForumModerate).await;
    press_full(&mut channel, "x").await;
    let screen = press_full(&mut channel, "y").await;
    assert!(screen.contains(refused), "{screen}");
    assert_eq!(count(&rig, "post").await, 2);
    rig.restore(&taken, Permission::ForumModerate).await;
    press_full(&mut channel, "x").await;
    let screen = press_full(&mut channel, "y").await;
    assert!(screen.contains("Removed."), "{screen}");
    assert_eq!(count(&rig, "post").await, 1);
    rig.server.shutdown().await;
}

#[tokio::test]
async fn mail_paths_follow_their_permissions_on_the_next_action() {
    const SEND: &str = "\u{18}";
    let rig = Rig::start().await;
    let key = new_client_key();
    let (_session, mut channel) = founder_at_menu(&rig, &key).await;
    let refused = "You may not do that.";
    let me: String = sqlx::query_scalar("SELECT username FROM accounts LIMIT 1")
        .fetch_one(&rig.db)
        .await
        .unwrap();

    // mail.send: a message is written only while it is held (the founder writes to themselves).
    press_full(&mut channel, "m").await;
    let taken = rig.revoke(Permission::MailSend).await;
    press_full(&mut channel, "c").await;
    press_full(&mut channel, &format!("{me}\r")).await;
    let screen = press_full(&mut channel, "Hello\r").await;
    assert!(screen.contains(refused), "{screen}");
    assert_eq!(count(&rig, "mail_message").await, 0, "nothing was written");
    rig.restore(&taken, Permission::MailSend).await;
    // The subject prompt is still open with its text; Enter tries again.
    let editor = press_full(&mut channel, "\r").await;
    assert!(editor.contains(&format!("To {me}: Hello")), "{editor}");
    press_full(&mut channel, "a note").await;
    // Taken away between opening the editor and sending: the send is refused, the text is kept.
    let taken = rig.revoke(Permission::MailSend).await;
    let screen = press_full(&mut channel, SEND).await;
    assert!(screen.contains(refused), "{screen}");
    assert_eq!(count(&rig, "mail_message").await, 0);
    rig.restore(&taken, Permission::MailSend).await;
    press_full(&mut channel, "c").await;
    press_full(&mut channel, &format!("{me}\r")).await;
    let editor = press_full(&mut channel, "Hello\r").await;
    assert!(
        editor.contains("a note"),
        "the refused text came back: {editor}"
    );
    assert_eq!(
        count(&rig, "draft").await,
        0,
        "mail drafts are never stored"
    );
    let screen = press_full(&mut channel, SEND).await;
    assert!(screen.contains("Sent."), "{screen}");
    assert_eq!(count(&rig, "mail_message").await, 1);

    // mail.read: a message that was listed is no longer opened, and it is not deleted either.
    let taken = rig.revoke(Permission::MailRead).await;
    let screen = press_full(&mut channel, "\r").await;
    assert!(screen.contains(refused), "{screen}");
    press_full(&mut channel, "d").await;
    let screen = press_full(&mut channel, "y").await;
    assert!(screen.contains(refused), "{screen}");
    assert_eq!(count(&rig, "mail_message").await, 1);
    rig.restore(&taken, Permission::MailRead).await;
    let screen = press_full(&mut channel, "\r").await;
    assert!(screen.contains("a note"), "{screen}");

    // mail.report: a copy goes to staff only while it is held.
    let taken = rig.revoke(Permission::MailReport).await;
    press_full(&mut channel, "x").await;
    let screen = press_full(&mut channel, "y").await;
    assert!(
        !screen.contains("The staff have a copy") && count(&rig, "mail_report").await == 0,
        "{screen}"
    );
    rig.restore(&taken, Permission::MailReport).await;
    press_full(&mut channel, "x").await;
    let screen = press_full(&mut channel, "y").await;
    assert!(screen.contains("The staff have a copy"), "{screen}");
    assert_eq!(count(&rig, "mail_report").await, 1);

    // mail.read again: deleting is refused while it is taken, and works when it is back.
    let taken = rig.revoke(Permission::MailRead).await;
    press_full(&mut channel, "d").await;
    let screen = press_full(&mut channel, "y").await;
    assert!(screen.contains(refused), "{screen}");
    assert_eq!(count(&rig, "mail_message").await, 1);
    rig.restore(&taken, Permission::MailRead).await;
    press_full(&mut channel, "d").await;
    let screen = press_full(&mut channel, "y").await;
    assert!(screen.contains("Deleted."), "{screen}");
    assert_eq!(count(&rig, "mail_message").await, 0);
    rig.server.shutdown().await;
}

// ---- community (M3-J) ----

#[tokio::test]
async fn posting_a_one_liner_needs_oneliners_post_and_a_removal_writes_nothing() {
    let rig = Rig::start().await;
    let (_alice, _session, mut channel) = member_at_menu(&rig, "alice").await;

    // Held: the menu offers Community, the screen offers the key, and a line is stored.
    assert!(full(&mut channel).await.contains("[y] Community"));
    send(&mut channel, "y").await;
    assert!(full(&mut channel).await.contains("[o] One liners"));
    send(&mut channel, "o").await;
    assert!(full(&mut channel).await.contains("p: add a line"));
    send(&mut channel, "p").await;
    send(&mut channel, "hello there\r").await;
    assert!(full(&mut channel).await.contains("1. alice: hello there"));
    assert_eq!(count(&rig, "community_line").await, 1);

    // Taken away while the line is being typed: refused in words, nothing written.
    send(&mut channel, "p").await;
    let taken = rig.revoke(Permission::OnelinersPost).await;
    let first = send(&mut channel, "sneaky\r").await;
    let screen = format!("{first} {}", full(&mut channel).await);
    assert!(
        screen.contains("You are not allowed to do that."),
        "{screen}"
    );
    assert_eq!(count(&rig, "community_line").await, 1, "nothing written");

    // The key is no longer offered and does nothing.
    send(&mut channel, "yo").await;
    assert!(!full(&mut channel).await.contains("p: add a line"));
    send(&mut channel, "p").await;
    assert!(!full(&mut channel).await.contains("Your line"));

    // Put back: it works on the next action.
    rig.restore(&taken, Permission::OnelinersPost).await;
    send(&mut channel, "x").await;
    assert!(full(&mut channel).await.contains("p: add a line"));
    send(&mut channel, "p").await;
    send(&mut channel, "second\r").await;
    assert_eq!(count(&rig, "community_line").await, 2);

    // `community.view` taken away while the screen is open: the next key closes it and says why.
    let view = rig.revoke(Permission::CommunityView).await;
    let first = send(&mut channel, "x").await;
    let screen = format!("{first} {}", full(&mut channel).await);
    assert!(
        screen.contains("You are not allowed to do that."),
        "{screen}"
    );
    assert!(!full(&mut channel).await.contains("[y] Community"));
    rig.restore(&view, Permission::CommunityView).await;
    rig.server.shutdown().await;
}

#[tokio::test]
async fn writing_on_the_wall_needs_graffiti_post_and_a_removal_writes_nothing() {
    let rig = Rig::start().await;
    let (_alice, _session, mut channel) = member_at_menu(&rig, "alice").await;
    send(&mut channel, "yg").await;
    assert!(full(&mut channel).await.contains("Graffiti wall"));
    send(&mut channel, "p").await;
    send(&mut channel, "was here\r").await;
    assert!(full(&mut channel).await.contains("1. alice: was here"));
    assert_eq!(count(&rig, "community_line").await, 1);
    send(&mut channel, "p").await;
    let taken = rig.revoke(Permission::GraffitiPost).await;
    let first = send(&mut channel, "nope\r").await;
    let screen = format!("{first} {}", full(&mut channel).await);
    assert!(
        screen.contains("You are not allowed to do that."),
        "{screen}"
    );
    assert_eq!(count(&rig, "community_line").await, 1, "nothing written");
    rig.restore(&taken, Permission::GraffitiPost).await;
    rig.server.shutdown().await;
}

#[tokio::test]
async fn voting_and_making_polls_need_their_permissions_and_a_second_vote_is_refused() {
    use russh_bbs_core::community::{
        CommunityStore, CreatePoll, NewPoll, PollRules, ResultsVisibility,
    };
    use russh_bbs_core::store::UnixSeconds;
    let rig = Rig::start().await;
    let (alice, _session, mut channel) = member_at_menu(&rig, "alice").await;
    let options = vec!["red".to_owned(), "blue".to_owned()];
    let poll = NewPoll::build("Best colour?", &options, "", ResultsVisibility::Always).unwrap();
    let rules = PollRules {
        max_polls: 90,
        per_day: 3,
    };
    let made = rig
        .store
        .create_poll(alice, &poll, rules, UnixSeconds::from_raw(1))
        .await
        .unwrap();
    assert!(matches!(made, CreatePoll::Created(_)));

    // A member cannot make polls: no hint, and the key does nothing.
    send(&mut channel, "yp").await;
    let list = full(&mut channel).await;
    assert!(
        list.contains("[1] Best colour?") && !list.contains("c: make a poll"),
        "{list}"
    );
    send(&mut channel, "c").await;
    assert!(!full(&mut channel).await.contains("New poll"));

    // Voting: offered, then taken away (nothing written), then put back and counted once.
    send(&mut channel, "1").await;
    assert!(full(&mut channel).await.contains("1 6: vote"));
    let taken = rig.revoke(Permission::PollsVote).await;
    send(&mut channel, "2").await;
    assert!(!full(&mut channel).await.contains("1 6: vote"));
    assert_eq!(count(&rig, "poll_vote").await, 0, "nothing written");
    rig.restore(&taken, Permission::PollsVote).await;
    send(&mut channel, "2").await;
    let after = full(&mut channel).await;
    assert!(after.contains("Your vote is counted."), "{after}");
    assert_eq!(count(&rig, "poll_vote").await, 1);
    // The second vote is refused in words and changes nothing.
    send(&mut channel, "1").await;
    let again = full(&mut channel).await;
    assert!(
        again.contains("You have already voted on this poll."),
        "{again}"
    );
    assert_eq!(count(&rig, "poll_vote").await, 1);

    // Making a poll: granted to members for this test, then taken away while the form is open.
    sqlx::query("INSERT INTO role_permission (role, permission) VALUES ('member', 'polls.create')")
        .execute(&rig.db)
        .await
        .unwrap();
    send(&mut channel, "\u{1b}").await;
    assert!(full(&mut channel).await.contains("c: make a poll"));
    send(&mut channel, "c").await;
    send(&mut channel, "Pick one?\ra\rb\r\r").await;
    assert!(full(&mut channel).await.contains("Enter: make the poll"));
    let taken = rig.revoke(Permission::PollsCreate).await;
    let first = send(&mut channel, "\r").await;
    let screen = format!("{first} {}", full(&mut channel).await);
    assert!(
        screen.contains("You are not allowed to do that."),
        "{screen}"
    );
    assert_eq!(count(&rig, "poll").await, 1, "nothing written");
    rig.restore(&taken, Permission::PollsCreate).await;
    send(&mut channel, "x").await;
    send(&mut channel, "ypc").await;
    send(&mut channel, "Pick one?\ra\rb\r\r").await;
    send(&mut channel, "\r").await;
    assert_eq!(count(&rig, "poll").await, 2);
    rig.server.shutdown().await;
}

#[tokio::test]
async fn moderating_needs_community_moderate_and_is_audited_and_last_callers_lists_members() {
    use russh_bbs_core::community::{AddLine, CommunityStore, LineKind, LineText};
    use russh_bbs_core::store::{AccountState, AccountStore, NewAccount, UnixSeconds};
    let rig = Rig::start().await;
    let (alice, _alice_session, mut alice_channel) = member_at_menu(&rig, "alice").await;
    let added = rig
        .store
        .add_line(
            alice,
            LineKind::Oneliner,
            &LineText::parse(LineKind::Oneliner, "remove me").unwrap(),
            UnixSeconds::from_raw(1),
        )
        .await
        .unwrap();
    assert!(matches!(added, AddLine::Added(_)));
    // A moderator.
    let key = new_client_key();
    let fp =
        Fingerprint::parse(&key.public_key().fingerprint(HashAlg::Sha256).to_string()).unwrap();
    let maude = rig
        .store
        .create_account(NewAccount {
            username: russh_bbs_core::sanitize::sanitize_username("maude").unwrap(),
            role: Slug::MODERATOR,
            state: AccountState::Active,
        })
        .await
        .unwrap();
    rig.store.attach_fingerprint(maude.id, &fp).await.unwrap();
    common::remember(fp, maude.username.as_str());
    let session = login(rig.addr(), "maude", &key).await;
    let mut channel = common::open_shell(&session, 80, 24).await;
    words(&mut channel).await;
    for key in ["x", "n", "y"] {
        send(&mut channel, key).await;
    }

    // Last callers: both sessions began, so both members are listed, newest first.
    send(&mut channel, "yl").await;
    let callers = full(&mut channel).await;
    assert!(
        callers.contains("Last callers") && callers.contains("alice") && callers.contains("maude"),
        "{callers}"
    );
    send(&mut channel, "\u{1b}").await;

    // Held: the moderator is offered the key; taken away: it is not, and nothing is removed.
    send(&mut channel, "o").await;
    assert!(
        full(&mut channel)
            .await
            .contains("d: remove a line (staff)")
    );
    sqlx::query("DELETE FROM role_permission WHERE permission = 'community.moderate'")
        .execute(&rig.db)
        .await
        .unwrap();
    send(&mut channel, "d").await;
    send(&mut channel, "1\r").await;
    let view = full(&mut channel).await;
    assert!(
        !view.contains("d: remove a line (staff)") && view.contains("remove me"),
        "{view}"
    );
    assert_eq!(count(&rig, "community_line").await, 1, "nothing removed");
    assert_eq!(
        count(&rig, "audit_log WHERE action = 'community.entry_removed'").await,
        0
    );

    // Put back: the removal happens and is audited.
    sqlx::query(
        "INSERT INTO role_permission (role, permission) VALUES ('moderator', 'community.moderate')",
    )
    .execute(&rig.db)
    .await
    .unwrap();
    send(&mut channel, "x").await;
    send(&mut channel, "d").await;
    send(&mut channel, "1\r").await;
    assert_eq!(count(&rig, "community_line").await, 0);
    assert_eq!(
        count(&rig, "audit_log WHERE action = 'community.entry_removed'").await,
        1
    );
    let _ = &mut alice_channel;
    rig.server.shutdown().await;
}

#[tokio::test]
async fn my_keys_needs_keys_manage_and_a_removal_closes_the_open_screen() {
    let rig = Rig::start().await;
    let (_alice, _session, mut channel) = member_at_menu(&rig, "alice").await;

    // Held: Members offers My keys, the screen opens, and a code is made.
    send(&mut channel, "u").await;
    assert!(full(&mut channel).await.contains("[k] My keys"));
    send(&mut channel, "k").await;
    assert!(full(&mut channel).await.contains("[l] Link a new machine"));
    send(&mut channel, "l").await;
    assert!(full(&mut channel).await.contains("Code:"));
    send(&mut channel, "x").await;
    assert_eq!(count(&rig, "login_code").await, 1);

    // Taken away while the screen is open: the next key closes it, says why, and makes no code.
    let taken = rig.revoke(Permission::KeysManage).await;
    let first = send(&mut channel, "l").await;
    let screen = format!("{first} {}", full(&mut channel).await);
    assert!(
        screen.contains("You are not allowed to do that."),
        "{screen}"
    );
    assert_eq!(count(&rig, "login_code").await, 1, "no code was made");
    send(&mut channel, "u").await;
    let menu = full(&mut channel).await;
    assert!(!menu.contains("[k] My keys"), "{menu}");
    send(&mut channel, "k").await;
    assert!(
        !full(&mut channel)
            .await
            .contains("Keys sign you in without a password")
    );

    // Put back: it works on the next action.
    rig.restore(&taken, Permission::KeysManage).await;
    send(&mut channel, "x").await;
    assert!(full(&mut channel).await.contains("[k] My keys"));
    send(&mut channel, "k").await;
    send(&mut channel, "l").await;
    assert!(full(&mut channel).await.contains("Code:"));
    assert_eq!(count(&rig, "login_code").await, 2);
    rig.server.shutdown().await;
}

#[tokio::test]
async fn redeeming_a_code_needs_keys_redeem_and_a_refusal_uses_nothing_up() {
    let rig = Rig::start().await;
    let (alice, _session, mut channel) = member_at_menu(&rig, "alice").await;
    send(&mut channel, "u").await;
    send(&mut channel, "k").await;
    send(&mut channel, "l").await;
    let shown = full(&mut channel).await;
    // `words` turns the dash into a space, so the two halves are put back together.
    let code = shown
        .split("Code: ")
        .nth(1)
        .map(|rest| {
            rest.split_whitespace()
                .take(2)
                .collect::<Vec<_>>()
                .join("-")
        })
        .unwrap();

    let machine = |rig: &Rig| {
        let addr = rig.addr();
        async move {
            let key = new_client_key();
            let session = login(addr, "new", &key).await;
            let mut channel = common::open_shell(&session, 80, 24).await;
            words(&mut channel).await;
            (session, channel, key)
        }
    };
    // `waiting` holds `keys.redeem`; take it away and a valid code is refused like a bad one.
    sqlx::query(
        "DELETE FROM role_permission WHERE role = 'waiting' AND permission = 'keys.redeem'",
    )
    .execute(&rig.db)
    .await
    .unwrap();
    let (_s, mut visitor, _key) = machine(&rig).await;
    send(&mut visitor, "d").await;
    send(&mut visitor, &format!("{code}\r")).await;
    let refused = full(&mut visitor).await;
    assert!(refused.contains("That code did not work."), "{refused}");
    assert_eq!(count(&rig, "account_keys").await, 1);
    assert_eq!(
        count(&rig, "login_code WHERE redeemed_at IS NOT NULL").await,
        0
    );

    // Put back: the same code works on the next attempt, so the refusal used nothing up.
    sqlx::query("INSERT INTO role_permission (role, permission) VALUES ('waiting', 'keys.redeem')")
        .execute(&rig.db)
        .await
        .unwrap();
    let (_s, mut visitor, _key) = machine(&rig).await;
    send(&mut visitor, "d").await;
    send(&mut visitor, &format!("{code}\r")).await;
    let joined = full(&mut visitor).await;
    assert!(joined.contains("Signed in as alice"), "{joined}");
    assert_eq!(count(&rig, "account_keys").await, 2);
    let _ = alice;
    rig.server.shutdown().await;
}

#[tokio::test]
async fn issuing_a_recovery_code_needs_keys_recover_and_a_removal_makes_none() {
    let rig = Rig::start().await;
    let key = new_client_key();
    let (_session, mut channel) = founder_at_menu(&rig, &key).await;
    let (_alice, _alice_session, _alice_channel) = member_at_menu(&rig, "alice").await;

    send(&mut channel, "o").await;
    assert!(full(&mut channel).await.contains("[r] Recovery code"));
    send(&mut channel, "r").await;
    send(&mut channel, "alice\r").await;
    assert!(full(&mut channel).await.contains("Code:"));
    assert_eq!(count(&rig, "login_code").await, 1);
    send(&mut channel, "x").await;

    // Taken away while the screen is open: the next key closes it and no code is made.
    let taken = rig.revoke(Permission::KeysRecover).await;
    let first = send(&mut channel, "a").await;
    let screen = format!("{first} {}", full(&mut channel).await);
    assert!(
        screen.contains("You are not allowed to do that."),
        "{screen}"
    );
    assert_eq!(count(&rig, "login_code").await, 1, "no code was made");
    send(&mut channel, "o").await;
    assert!(!full(&mut channel).await.contains("[r] Recovery code"));

    // Put back: it works on the next action.
    rig.restore(&taken, Permission::KeysRecover).await;
    send(&mut channel, "x").await;
    assert!(full(&mut channel).await.contains("[r] Recovery code"));
    rig.server.shutdown().await;
}

#[tokio::test]
async fn doors_run_gates_the_menu_item_and_an_open_door_stops_saving_when_it_is_taken_away() {
    let rig = Rig::start().await;
    let key = new_client_key();
    let (_session, mut channel) = founder_at_menu(&rig, &key).await;

    send(&mut channel, "d").await;
    let list = full(&mut channel).await;
    assert!(list.contains("Coin Toss"), "{list}");
    send(&mut channel, "\r").await;
    assert!(full(&mut channel).await.contains("Tosses left today"));
    send(&mut channel, "h").await;
    assert_eq!(count(&rig, "door_save").await, 1);
    let revision = || async {
        sqlx::query_scalar::<_, i64>("SELECT revision FROM door_save")
            .fetch_one(&rig.db)
            .await
            .unwrap()
    };
    assert_eq!(revision().await, 1);

    // Taken away while the door is open: the next toss is refused, nothing more is written, and
    // the door closes with the plain sentence.
    let taken = rig.revoke(Permission::DoorsRun).await;
    send(&mut channel, "h").await;
    let screen = full(&mut channel).await;
    assert!(
        screen.contains("You do not have access to doors."),
        "{screen}"
    );
    assert_eq!(revision().await, 1, "the refused toss was not saved");
    // Back at the list, then the menu: the item is gone and its hotkey does nothing.
    send(&mut channel, "\u{1b}").await;
    let menu = full_until(&mut channel, "Main menu").await;
    assert!(
        menu.contains("Main menu") && !menu.contains("[d] Doors"),
        "{menu}"
    );
    send(&mut channel, "d").await;
    assert!(!full(&mut channel).await.contains("Pick a door"));

    // Put back: it works on the next action.
    rig.restore(&taken, Permission::DoorsRun).await;
    send(&mut channel, "x").await;
    assert!(full(&mut channel).await.contains("[d] Doors"));
    send(&mut channel, "d").await;
    assert!(full(&mut channel).await.contains("Pick a door"));
    rig.server.shutdown().await;
}
