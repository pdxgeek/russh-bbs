//! Chat rooms, presence and whispers over a real russh client against the real SQLite store
//! (M3-F, D84, D78): three members chat at once, a line reaches the others without a key press,
//! a whisper reaches only its target, a member's connection ends mid-line, a flood is refused,
//! and every chat permission is taken away from its role and put back (the access-path tests).
//! Staff open and close a room and the audit log says so; nothing said is written anywhere.
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
use russh_bbs_core::access::{Fingerprint, Permission, Slug};
use russh_bbs_core::icons::Tier;
use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::store::{AccountId, AccountState, AccountStore, NewAccount};
use russh_bbs_core::terminal_settings::{TerminalSettings, TerminalSettingsStore};
use russh_bbs_server::ssh::{HostKey, Limits, ServerConfig, SshServer};
use russh_bbs_store::SqliteStore;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};
use tempfile::TempDir;
use tokio::time::timeout;

struct Rig {
    server: SshServer,
    store: SqliteStore,
    db: SqlitePool,
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
        let server = SshServer::start(&config, &host_key, Arc::new(store.clone()))
            .await
            .unwrap();
        Self {
            server,
            store,
            db,
            _dir: dir,
        }
    }

    fn addr(&self) -> SocketAddr {
        self.server.local_addr()
    }

    /// An account with a key. Terminal settings already say "ASCII icons", so login goes
    /// straight to the menu.
    async fn account(
        &self,
        name: &str,
        role: Slug,
        state: AccountState,
    ) -> (AccountId, PrivateKey) {
        let key = new_client_key();
        let account = self
            .store
            .create_account(NewAccount {
                username: sanitize_username(name).unwrap(),
                role,
                state,
            })
            .await
            .unwrap();
        let fp =
            Fingerprint::parse(&key.public_key().fingerprint(HashAlg::Sha256).to_string()).unwrap();
        self.store
            .attach_fingerprint(account.id, &fp)
            .await
            .unwrap();
        common::remember(fp, account.username.as_str());
        let settings = TerminalSettings::new(None, None, true, true, Some(Tier::Ascii)).unwrap();
        self.store
            .set_terminal_settings(account.id, &settings)
            .await
            .unwrap();
        (account.id, key)
    }

    async fn member(&self, name: &str) -> (AccountId, PrivateKey) {
        self.account(name, Slug::MEMBER, AccountState::Active).await
    }

    /// Take `permission` away from the roles that carry it (stands in for a role edit, which is
    /// a later slice). Returns the roles, for `restore`.
    async fn revoke(&self, permission: Permission, roles: &[&str]) -> Vec<String> {
        let mut taken = Vec::new();
        for role in roles {
            let done = sqlx::query("DELETE FROM role_permission WHERE role = ? AND permission = ?")
                .bind(role)
                .bind(permission.name())
                .execute(&self.db)
                .await
                .unwrap();
            if done.rows_affected() > 0 {
                taken.push((*role).to_owned());
            }
        }
        assert!(!taken.is_empty(), "{} was held by none", permission.name());
        taken
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
}

async fn login(addr: SocketAddr, key: &PrivateKey) -> Handle<Client> {
    let mut session = common::connect(addr).await;
    let result = session
        .authenticate_publickey(
            &common::user_for(key, "anyone"),
            PrivateKeyWithHashAlg::new(Arc::new(key.clone()), None),
        )
        .await
        .unwrap();
    assert!(matches!(result, AuthResult::Success), "{result:?}");
    session
}

/// Everything the channel sends until it is quiet, exactly as received.
async fn raw(channel: &mut Channel<client::Msg>) -> String {
    let mut raw = String::new();
    while let Ok(msg) = timeout(Duration::from_millis(250), channel.wait()).await {
        match msg {
            Some(ChannelMsg::Data { data }) => raw.push_str(&String::from_utf8_lossy(&data)),
            Some(ChannelMsg::Close | ChannelMsg::Eof) | None => break,
            Some(_) => {}
        }
    }
    raw
}

/// The printable words of what was received (escape sequences and box drawing removed).
fn words_of(raw: &str) -> String {
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

async fn words(channel: &mut Channel<client::Msg>) -> String {
    words_of(&raw(channel).await)
}

async fn send(channel: &mut Channel<client::Msg>, bytes: &str) -> String {
    channel.data_bytes(bytes.as_bytes().to_vec()).await.unwrap();
    words(channel).await
}

/// The whole current screen: a window change makes the server clear and redraw everything.
async fn full(channel: &mut Channel<client::Msg>) -> String {
    channel.window_change(80, 24, 0, 0).await.unwrap();
    words(channel).await
}

/// Log in and get past the login screen to the main menu.
async fn at_menu(rig: &Rig, key: &PrivateKey) -> (Handle<Client>, Channel<client::Msg>) {
    let session = login(rig.addr(), key).await;
    let mut channel = common::open_shell(&session, 80, 24).await;
    words(&mut channel).await;
    send(&mut channel, "x").await;
    (session, channel)
}

/// Read until `needle` appears, with no key sent: what a push looks like to the client.
async fn until(channel: &mut Channel<client::Msg>, needle: &str) -> String {
    let mut seen = String::new();
    let found = timeout(Duration::from_secs(8), async {
        while !words_of(&seen).contains(&words_of(needle)) {
            match channel.wait().await {
                Some(ChannelMsg::Data { data }) => seen.push_str(&String::from_utf8_lossy(&data)),
                Some(_) => {}
                None => return false,
            }
        }
        true
    })
    .await;
    assert!(
        found == Ok(true),
        "never saw {needle:?}; the screen said: {}",
        words_of(&seen)
    );
    seen
}

/// Join the lobby from the main menu.
async fn into_the_lobby(channel: &mut Channel<client::Msg>) {
    send(channel, "c").await;
    let list = full(channel).await;
    assert!(
        list.contains("Chat rooms") && list.contains("lobby"),
        "{list}"
    );
    send(channel, "\r").await;
    let room = full(channel).await;
    assert!(
        room.contains("#lobby") && room.contains("You are in #lobby."),
        "{room}"
    );
}

#[tokio::test]
async fn three_members_chat_with_a_push_a_whisper_a_flood_and_a_leaver() {
    let rig = Rig::start().await;
    let mut people = Vec::new();
    for name in ["ann", "bob", "cy"] {
        let (_, key) = rig.member(name).await;
        let (session, mut channel) = at_menu(&rig, &key).await;
        assert!(full(&mut channel).await.contains("[c] Chat"));
        into_the_lobby(&mut channel).await;
        people.push((session, channel));
    }
    let (mut cy, mut bob, mut ann) = (
        people.pop().unwrap(),
        people.pop().unwrap(),
        people.pop().unwrap(),
    );

    // The others are told, without a key, who came in.
    let seen_by_ann = until(&mut ann.1, "cy joined").await;
    assert!(seen_by_ann.contains("cy joined"));
    assert!(full(&mut cy.1).await.contains("Here: ann, bob, cy"));

    // 1. A line reaches the others with no key pressed on their side.
    ann.1.data_bytes(b"helloeveryone\r".to_vec()).await.unwrap();
    assert!(
        until(&mut bob.1, "<ann> helloeveryone")
            .await
            .contains("helloeveryone")
    );
    assert!(
        until(&mut cy.1, "<ann> helloeveryone")
            .await
            .contains("helloeveryone")
    );
    assert!(
        until(&mut ann.1, "<ann> helloeveryone")
            .await
            .contains("helloeveryone")
    );

    // 2. A whisper reaches its target and echoes to the sender; the third member sees nothing.
    cy.1.data_bytes(b"/w Ann theplanisfish\r".to_vec())
        .await
        .unwrap();
    assert!(
        until(&mut ann.1, "cy whispers: theplanisfish")
            .await
            .contains("whispers")
    );
    assert!(
        until(&mut cy.1, "you whisper to ann: theplanisfish")
            .await
            .contains("whisper")
    );
    assert!(!words(&mut bob.1).await.contains("theplanisfish"));
    assert!(!full(&mut bob.1).await.contains("theplanisfish"));

    // 3. A whisper to a handle that is not here is "Not here." whether or not it exists.
    cy.1.data_bytes(b"/w nobody hi\r".to_vec()).await.unwrap();
    until(&mut cy.1, "Not here.").await;
    rig.member("dee").await; // exists, not in the room
    // The notice is identical to the last one, so the frame is too: ask for a repaint to see it.
    cy.1.data_bytes(b"/w dee hi\r".to_vec()).await.unwrap();
    assert!(full(&mut cy.1).await.contains("Not here."));

    // 4. Hostile text is neutralized before anyone sees it.
    ann.1
        .data_bytes(b"evil \x1b[31mred\x1b[0m \x1b]0;pwn\x07 text\r".to_vec())
        .await
        .unwrap();
    let screen = until(&mut bob.1, "<ann> evil").await;
    assert!(
        !screen.contains("\u{1b}]0;"),
        "an OSC sequence reached bob: {screen:?}"
    );
    assert!(full(&mut bob.1).await.contains("evil"));

    // 5. A flood is refused, and does not reach the room.
    bob.1
        .data_bytes("f\rf\rf\rf\rf\rf\rf\rf\rf\rf\rf\rf\r".as_bytes().to_vec())
        .await
        .unwrap();
    until(&mut bob.1, "Slow down").await;

    // 6. cy's connection ends in the middle of a line: nothing of it is ever said, and the
    //    others are told he left.
    cy.1.data_bytes(b"halftypedneversent".to_vec())
        .await
        .unwrap();
    full(&mut cy.1).await;
    cy.0.disconnect(russh::Disconnect::ByApplication, "", "en")
        .await
        .unwrap();
    drop(cy);
    assert!(until(&mut ann.1, "cy left").await.contains("cy left"));
    assert!(until(&mut bob.1, "cy left").await.contains("cy left"));
    assert!(!full(&mut ann.1).await.contains("halftyped"));
    assert!(!full(&mut bob.1).await.contains("halftyped"));

    // 7. Esc goes back through the list to the menu; chat is left cleanly.
    send(&mut ann.1, "\x1b").await;
    assert!(full(&mut ann.1).await.contains("Chat rooms"));
    send(&mut ann.1, "\x1b").await;
    assert!(full(&mut ann.1).await.contains("Main menu"));
    assert!(until(&mut bob.1, "ann left").await.contains("ann left"));
}

#[tokio::test]
async fn nothing_said_is_stored_and_the_waiting_room_cannot_reach_chat() {
    let rig = Rig::start().await;
    let (_, ann_key) = rig.member("ann").await;
    let (_, bob_key) = rig.member("bob").await;
    let (_a, mut ann) = at_menu(&rig, &ann_key).await;
    let (_b, mut bob) = at_menu(&rig, &bob_key).await;
    into_the_lobby(&mut ann).await;
    into_the_lobby(&mut bob).await;
    ann.data_bytes(b"CANARYLINE1\r".to_vec()).await.unwrap();
    until(&mut bob, "CANARYLINE1").await;
    ann.data_bytes(b"/w bob CANARYWHISPER2\r".to_vec())
        .await
        .unwrap();
    until(&mut bob, "CANARYWHISPER2").await;

    // Every table's text, dumped from the database the server is using.
    let tables: Vec<String> =
        sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type = 'table'")
            .fetch_all(&rig.db)
            .await
            .unwrap();
    for table in tables {
        let rows = sqlx::query(&format!("SELECT * FROM \"{table}\""))
            .fetch_all(&rig.db)
            .await
            .unwrap();
        for row in rows {
            use sqlx::{Column, Row};
            for column in row.columns() {
                if let Ok(text) = row.try_get::<String, _>(column.ordinal()) {
                    assert!(
                        !text.contains("CANARY"),
                        "{table}.{} holds chat text",
                        column.name()
                    );
                }
            }
        }
    }
    let audit: Vec<String> =
        sqlx::query_scalar("SELECT action FROM audit_log WHERE action LIKE 'chat.%'")
            .fetch_all(&rig.db)
            .await
            .unwrap();
    assert_eq!(audit.len(), 0, "saying and whispering write no audit row");

    // A key the board does not know gets the waiting room, and `c` does nothing there.
    let stranger = new_client_key();
    let session = login(rig.addr(), &stranger).await;
    let mut channel = common::open_shell(&session, 80, 24).await;
    let before = words(&mut channel).await;
    assert!(!before.contains("Chat"), "{before}");
    let after = send(&mut channel, "c").await;
    assert!(
        !after.contains("Chat rooms") && !after.contains("lobby"),
        "{after}"
    );
}

#[tokio::test]
async fn every_chat_permission_is_enforced_on_the_next_action() {
    let rig = Rig::start().await;
    let (_, key) = rig.member("ann").await;
    let (_other, other_key) = rig.member("bob").await;
    let (_s, mut ann) = at_menu(&rig, &key).await;
    let (_t, mut bob) = at_menu(&rig, &other_key).await;
    into_the_lobby(&mut ann).await;
    into_the_lobby(&mut bob).await;

    // chat.speak: without it a line is refused and nobody hears it.
    let speak = rig.revoke(Permission::ChatSpeak, &["member"]).await;
    ann.data_bytes(b"mutedline\r".to_vec()).await.unwrap();
    until(&mut ann, "You may not do that.").await;
    assert!(!words(&mut bob).await.contains("mutedline"));
    rig.restore(&speak, Permission::ChatSpeak).await;
    ann.data_bytes(b"audibleline\r".to_vec()).await.unwrap();
    until(&mut bob, "<ann> audibleline").await;

    // chat.whisper: the same.
    let whisper = rig.revoke(Permission::ChatWhisper, &["member"]).await;
    ann.data_bytes(b"/w bob quietwords\r".to_vec())
        .await
        .unwrap();
    until(&mut ann, "You may not do that.").await;
    assert!(!words(&mut bob).await.contains("quietwords"));
    rig.restore(&whisper, Permission::ChatWhisper).await;
    ann.data_bytes(b"/w bob loudwords\r".to_vec())
        .await
        .unwrap();
    until(&mut bob, "ann whispers: loudwords").await;

    // chat.join: taking it puts the member out of the room on the next action and the menu
    // stops offering Chat; putting it back brings both back.
    let join = rig.revoke(Permission::ChatJoin, &["member"]).await;
    ann.data_bytes(b"afterrevoke\r".to_vec()).await.unwrap();
    let list = full(&mut ann).await;
    assert!(list.contains("There are no rooms"), "{list}");
    assert!(!words(&mut bob).await.contains("afterrevoke"));
    send(&mut ann, "\x1b").await;
    let menu = full(&mut ann).await;
    assert!(
        menu.contains("Main menu") && !menu.contains("[c] Chat"),
        "{menu}"
    );
    send(&mut ann, "c").await;
    assert!(!full(&mut ann).await.contains("Chat rooms"));
    rig.restore(&join, Permission::ChatJoin).await;
    send(&mut ann, "x").await; // any key: the next action loads what the role holds now
    let menu = full(&mut ann).await;
    assert!(menu.contains("[c] Chat"), "{menu}");
}

#[tokio::test]
async fn only_room_managers_open_and_close_rooms_and_each_is_audited() {
    let rig = Rig::start().await;
    let (_, member_key) = rig.member("mia").await;
    let (_, sysop_key) = rig.account("sam", Slug::SYSOP, AccountState::Active).await;

    // A plain member is offered no room management and its keys do nothing.
    let (_m, mut mia) = at_menu(&rig, &member_key).await;
    send(&mut mia, "c").await;
    let list = full(&mut mia).await;
    assert!(
        !list.contains("n new room") && list.contains("lobby"),
        "{list}"
    );
    send(&mut mia, "n").await;
    assert!(!full(&mut mia).await.contains("New room name"));

    // Staff open "games" and close it again.
    let (_s, mut sam) = at_menu(&rig, &sysop_key).await;
    send(&mut sam, "c").await;
    let hints = full(&mut sam).await;
    assert!(
        hints.contains("n new room") && hints.contains("x close room"),
        "{hints}"
    );
    send(&mut sam, "n").await;
    assert!(full(&mut sam).await.contains("New room name"));
    send(&mut sam, "Games2\r").await;
    let list = full(&mut sam).await;
    assert!(
        list.contains("Room opened.") && list.contains("games2"),
        "{list}"
    );
    // Mia sees the new room at her next key (a list is fetched on demand: Esc and back in).
    send(&mut mia, "\x1b").await;
    send(&mut mia, "c").await;
    assert!(full(&mut mia).await.contains("games2"));
    // Mia joins it; staff close it; Mia is put out and told by the list, without a key.
    send(&mut mia, "\r").await; // the list is by name: games2 first
    assert!(full(&mut mia).await.contains("#games2"));
    send(&mut sam, "x").await;
    assert!(full(&mut sam).await.contains("Close this room"));
    send(&mut sam, "y").await;
    assert!(full(&mut sam).await.contains("Room closed."));
    // The list comes back by itself (a diff redraw, so match a word only the list has).
    let pushed = until(&mut mia, "lobby").await;
    assert!(!pushed.contains("games2"));
    assert!(!full(&mut mia).await.contains("games2"));

    let rows: Vec<String> = sqlx::query_scalar(
        "SELECT action || '|' || resource_kind || '|' || detail FROM audit_log WHERE action LIKE 'chat.%' ORDER BY id",
    )
    .fetch_all(&rig.db)
    .await
    .unwrap();
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert!(
        rows[0].starts_with("chat.room_created|room|name games2"),
        "{rows:?}"
    );
    assert!(rows[1].starts_with("chat.room_closed|room|"), "{rows:?}");
}
