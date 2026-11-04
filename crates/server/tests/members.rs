//! The members list, profiles and the profile form over a real russh client against the real
//! SQLite store (M2-F): who sees what, what a member and staff may edit, that hostile text is
//! cleaned, and that a key that is not a member learns nothing.
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
use russh_bbs_core::profiles::{Field, Profile, ProfileStore, RawProfile};
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

    async fn profile_of(&self, id: AccountId) -> Profile {
        self.store.member(id).await.unwrap().unwrap().profile
    }

    async fn set_profile(&self, id: AccountId, raw: RawProfile) {
        self.store
            .save_profile(id, &Profile::from_raw(&raw), None)
            .await
            .unwrap();
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

/// Sign in as a visitor to the waiting room (`new`), whatever the key is bound to.
async fn visitor(addr: SocketAddr, key: &PrivateKey) -> Handle<Client> {
    let mut session = common::connect(addr).await;
    let result = session
        .authenticate_publickey(
            "new",
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

#[tokio::test]
async fn a_member_browses_the_list_searches_and_opens_a_profile() {
    let rig = Rig::start().await;
    let (alice, key) = rig.member("alice").await;
    let (bob, _) = rig.member("bob").await;
    rig.set_profile(
        alice,
        RawProfile {
            display_name: "Alice A.".into(),
            ..RawProfile::default()
        },
    )
    .await;
    rig.set_profile(
        bob,
        RawProfile {
            display_name: "Bob B.".into(),
            pronouns: "he/him".into(),
            location: "Oslo".into(),
            about: "Likes modems.".into(),
            website: "http://example.org/never_fetched".into(),
        },
    )
    .await;
    // Accounts that are not active members must not appear anywhere.
    for (name, state) in [
        ("pending_pat", AccountState::Pending),
        ("approved_ann", AccountState::Approved),
        ("suspended_sam", AccountState::Suspended),
    ] {
        let (id, _) = rig.account(name, Slug::MEMBER, state).await;
        rig.set_profile(
            id,
            RawProfile {
                display_name: "Hidden".into(),
                ..RawProfile::default()
            },
        )
        .await;
    }
    let (_session, mut channel) = at_menu(&rig, &key).await;

    assert!(full(&mut channel).await.contains("[u] Members"));
    send(&mut channel, "u").await;
    let menu = full(&mut channel).await;
    for item in ["[l] Member list", "[p] My profile", "[e] Edit my profile"] {
        assert!(menu.contains(item), "{item}: {menu}");
    }

    // The list: active members only, in handle order, with display names.
    send(&mut channel, "l").await;
    let list = full(&mut channel).await;
    assert!(list.contains("1. alice Alice A."), "{list}");
    assert!(list.contains("2. bob Bob B."), "{list}");
    assert!(list.contains("Page 1"), "{list}");
    for hidden in ["pending_pat", "approved_ann", "suspended_sam", "Hidden"] {
        assert!(!list.contains(hidden), "{hidden} leaked: {list}");
    }
    // D93: no server-readable status row.
    assert!(!list.contains("SERVER-READABLE"), "{list}");

    // Open a profile; every field is plain text, the website included.
    send(&mut channel, "2").await;
    let profile = full(&mut channel).await;
    for expected in [
        "Handle: bob",
        "Display name: Bob B.",
        "Pronouns: he/him",
        "Location: Oslo",
        "About: Likes modems.",
        "Website: http://example.org/never_fetched",
    ] {
        assert!(profile.contains(expected), "{expected}: {profile}");
    }
    // Esc returns to the same list; the plain member has no edit key on someone else's profile.
    assert!(!profile.contains("e: edit"), "{profile}");
    send(&mut channel, "\u{1b}").await;
    assert!(full(&mut channel).await.contains("2. bob"));

    // Search by the start of a handle.
    send(&mut channel, "/b").await;
    assert!(
        full(&mut channel)
            .await
            .contains("Handles starting with: b_")
    );
    send(&mut channel, "\r").await;
    let found = full(&mut channel).await;
    assert!(
        found.contains("1. bob") && !found.contains("alice"),
        "{found}"
    );
    send(&mut channel, "/zzz\r").await;
    assert!(
        full(&mut channel)
            .await
            .contains("No member's handle starts with that.")
    );
    send(&mut channel, "c").await;
    assert!(full(&mut channel).await.contains("1. alice"));

    // Esc leaves the list, then the submenu.
    send(&mut channel, "\u{1b}").await;
    assert!(full(&mut channel).await.contains("Member list"));
    rig.server.shutdown().await;
}

#[tokio::test]
async fn a_long_list_pages_forward_and_back_without_skipping() {
    let rig = Rig::start().await;
    let (_, key) = rig.member("user00").await;
    for n in 1..14 {
        rig.member(&format!("user{n:02}")).await;
    }
    let (_session, mut channel) = at_menu(&rig, &key).await;
    send(&mut channel, "ul").await;
    let first = full(&mut channel).await;
    assert!(
        first.contains("1. user00") && first.contains("9. user08"),
        "{first}"
    );
    assert!(!first.contains("user09"));
    send(&mut channel, "n").await;
    let second = full(&mut channel).await;
    assert!(
        second.contains("1. user09") && second.contains("5. user13"),
        "{second}"
    );
    assert!(second.contains("Page 2"), "{second}");
    assert!(!second.contains("user08"));
    // `n` on the last page does nothing; `p` goes back.
    send(&mut channel, "n").await;
    assert!(full(&mut channel).await.contains("Page 2"));
    send(&mut channel, "p").await;
    let back = full(&mut channel).await;
    assert!(
        back.contains("Page 1") && back.contains("1. user00"),
        "{back}"
    );
    rig.server.shutdown().await;
}

#[tokio::test]
async fn a_member_edits_their_own_profile_and_the_form_is_honest() {
    let rig = Rig::start().await;
    let (alice, key) = rig.member("alice").await;
    let (_session, mut channel) = at_menu(&rig, &key).await;

    send(&mut channel, "ue").await;
    let form = full(&mut channel).await;
    assert!(
        form.contains("The sysop and every member can read this profile."),
        "{form}"
    );
    assert!(form.contains("Plain text only. A website is never opened or made a link."));
    let lower = form.to_lowercase();
    for forbidden in ["private", "encrypted", "\u{1f512}"] {
        assert!(!lower.contains(forbidden), "{forbidden}: {form}");
    }
    assert!(form.contains("> Display name:"), "{form}");

    // Type each field and Enter to move on; the last Enter saves. A zero-width character and
    // a character beyond the cap must not reach the store.
    send(&mut channel, "Alice\u{200b} A.\r").await;
    send(&mut channel, "she/her\r").await;
    send(&mut channel, "Portland\r").await;
    send(&mut channel, &format!("{}\r", "a".repeat(400))).await;
    send(&mut channel, "http://169.254.169.254/\r").await;
    let saved = full(&mut channel).await;
    assert!(saved.contains("Profile saved."), "{saved}");
    assert!(saved.contains("Display name: Alice A."), "{saved}");

    let stored = rig.profile_of(alice).await;
    assert_eq!(stored.get(Field::DisplayName), "Alice A.");
    assert_eq!(stored.get(Field::Pronouns), "she/her");
    assert_eq!(stored.get(Field::Location), "Portland");
    assert_eq!(stored.get(Field::About).chars().count(), 280);
    assert_eq!(stored.get(Field::Website), "http://169.254.169.254/");

    // Esc in the form discards.
    send(&mut channel, "\u{1b}").await;
    send(&mut channel, "e").await;
    send(&mut channel, "ZZZ\u{1b}").await;
    assert_eq!(
        rig.profile_of(alice).await.get(Field::DisplayName),
        "Alice A.",
        "an abandoned edit writes nothing"
    );
    rig.server.shutdown().await;
}

#[tokio::test]
async fn staff_edit_another_profile_and_a_member_cannot() {
    let rig = Rig::start().await;
    let (_, member_key) = rig.member("mallory").await;
    let (bob, _) = rig.member("bob").await;
    rig.set_profile(
        bob,
        RawProfile {
            location: "Oslo".into(),
            ..RawProfile::default()
        },
    )
    .await;
    let (_, staff_key) = rig
        .account("carol", Slug::COSYSOP, AccountState::Active)
        .await;

    // A plain member: no edit key on Bob's profile, and pressing `e` opens nothing.
    let (_s1, mut member) = at_menu(&rig, &member_key).await;
    send(&mut member, "ul").await;
    send(&mut member, "1").await; // bob sorts first
    let view = full(&mut member).await;
    assert!(
        view.contains("Handle: bob") && !view.contains("e: edit"),
        "{view}"
    );
    send(&mut member, "e").await;
    let after = full(&mut member).await;
    assert!(
        !after.contains("> Display name:") && !after.contains("staff edit"),
        "{after}"
    );

    // Staff: the key is offered, the form names whose profile it is, and the save is stored.
    let (_s2, mut staff) = at_menu(&rig, &staff_key).await;
    send(&mut staff, "ul").await;
    send(&mut staff, "1").await;
    assert!(full(&mut staff).await.contains("e: edit"));
    send(&mut staff, "e").await;
    let form = full(&mut staff).await;
    assert!(
        form.contains("Editing the profile of bob (staff edit)"),
        "{form}"
    );
    send(&mut staff, "\r\r").await; // keep display name and pronouns
    // Location starts as "Oslo": erase it and type the new one; about and website stay; save.
    send(&mut staff, "\u{7f}\u{7f}\u{7f}\u{7f}Elsewhere\r\r\r").await;
    let saved = full(&mut staff).await;
    assert!(
        saved.contains("Profile saved.") && saved.contains("Location: Elsewhere"),
        "{saved}"
    );
    assert_eq!(rig.profile_of(bob).await.get(Field::Location), "Elsewhere");
    // The staff edit is in the audit log, with field names and no values; Bob's own kind of
    // edit (none made here) would not be.
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT action, detail FROM audit_log WHERE action = 'profile.edited'")
            .fetch_all(&rig.db)
            .await
            .unwrap();
    assert_eq!(
        rows,
        [("profile.edited".to_owned(), "fields location".to_owned())]
    );
    rig.server.shutdown().await;
}

#[tokio::test]
async fn a_cosysop_cannot_edit_a_sysops_profile_but_can_edit_a_members() {
    let rig = Rig::start().await;
    // Handles sort: aaron_sysop, carol, dina.
    let (sysop, _) = rig
        .account("aaron_sysop", Slug::SYSOP, AccountState::Active)
        .await;
    let (_, staff_key) = rig
        .account("carol", Slug::COSYSOP, AccountState::Active)
        .await;
    let (dina, _) = rig.member("dina").await;
    let sysop_before = rig.profile_of(sysop).await;

    let (_s, mut staff) = at_menu(&rig, &staff_key).await;
    send(&mut staff, "ul").await;
    send(&mut staff, "1").await;
    let view = full(&mut staff).await;
    assert!(view.contains("Handle: aaron_sysop"), "{view}");
    send(&mut staff, "e").await;
    let refused = full(&mut staff).await;
    assert!(
        refused.contains("You cannot edit the profile of someone whose role ranks at or above yours. Nothing was changed.")
            && !refused.contains("> Display name:"),
        "{refused}"
    );
    // The rank rule is the whole story: nothing was written and nothing was audited.
    assert_eq!(rig.profile_of(sysop).await, sysop_before);
    let audited: (i64,) =
        sqlx::query_as("SELECT count(*) FROM audit_log WHERE action = 'profile.edited'")
            .fetch_one(&rig.db)
            .await
            .unwrap();
    assert_eq!(audited.0, 0);

    // A member below the cosysop still works, and is audited.
    send(&mut staff, "\u{1b}").await;
    send(&mut staff, "3").await;
    assert!(full(&mut staff).await.contains("Handle: dina"));
    send(&mut staff, "e").await;
    let form = full(&mut staff).await;
    assert!(
        form.contains("Editing the profile of dina (staff edit)"),
        "{form}"
    );
    send(&mut staff, "Dina D\r\r\r\r\r").await;
    let saved = full(&mut staff).await;
    assert!(saved.contains("Profile saved."), "{saved}");
    assert_eq!(rig.profile_of(dina).await.get(Field::DisplayName), "Dina D");
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT action, detail FROM audit_log WHERE action = 'profile.edited'")
            .fetch_all(&rig.db)
            .await
            .unwrap();
    assert_eq!(
        rows,
        [(
            "profile.edited".to_owned(),
            "fields display_name".to_owned()
        )]
    );
    rig.server.shutdown().await;
}

#[tokio::test]
async fn stored_text_that_was_edited_by_hand_is_cleaned_on_render() {
    let rig = Rig::start().await;
    let (_, key) = rig.member("alice").await;
    let (bob, _) = rig.member("bob").await;
    // Straight into the table, as a hand edit or a bug would: nothing here goes through core.
    sqlx::query(
        "INSERT INTO profiles (account_id, display_name, pronouns, location, about, website)
         VALUES (?, ?, '', ?, ?, '')",
    )
    .bind(bob.as_raw())
    .bind("\u{1b}]0;pwned-title\u{7}Bobby\u{27e6}")
    .bind("\u{1b}[2Jcleared")
    .bind("line one\nline two\u{202e}rtl\u{e000}")
    .execute(&rig.db)
    .await
    .unwrap();
    let (_session, mut channel) = at_menu(&rig, &key).await;
    channel.data_bytes(b"ul".to_vec()).await.unwrap();
    let list = raw(&mut channel).await;
    channel.window_change(80, 24, 0, 0).await.unwrap();
    let list = format!("{list}{}", raw(&mut channel).await);
    channel.data_bytes(b"2".to_vec()).await.unwrap();
    let profile = raw(&mut channel).await;
    channel.window_change(80, 24, 0, 0).await.unwrap();
    let profile = format!("{profile}{}", raw(&mut channel).await);
    for stream in [&list, &profile] {
        assert!(
            !stream.contains("pwned-title"),
            "an OSC sequence got through"
        );
        assert!(!stream.contains('\u{202e}') && !stream.contains('\u{27e6}'));
        assert!(!stream.contains('\u{e000}'));
    }
    let words = words_of(&profile);
    assert!(words.contains("Display name: Bobby"), "{words}");
    assert!(words.contains("Location: cleared"), "{words}");
    assert!(words.contains("About: line one line twortl"), "{words}");
    rig.server.shutdown().await;
}

#[tokio::test]
async fn a_key_that_is_not_a_member_learns_nothing_about_members() {
    let rig = Rig::start().await;
    let (bob, bob_key) = rig.member("bob").await;
    rig.set_profile(
        bob,
        RawProfile {
            display_name: "Bobs Secret Name".into(),
            ..RawProfile::default()
        },
    )
    .await;
    rig.account("pending_pat", Slug::MEMBER, AccountState::Pending)
        .await;
    let (_, suspended_key) = rig
        .account("suspended_sam", Slug::MEMBER, AccountState::Suspended)
        .await;
    drop(bob_key);

    // An unknown key and a suspended member's key both see the waiting room, whatever they
    // press: no members menu, no list, no profile.
    let stranger = new_client_key();
    for key in [&stranger, &suspended_key] {
        let session = visitor(rig.addr(), key).await;
        let mut channel = common::open_shell(&session, 80, 24).await;
        let mut seen = words(&mut channel).await;
        for typed in ["u", "l", "1", "p", "e", "/", "b", "\r", "\u{1b}"] {
            seen.push(' ');
            seen.push_str(&send(&mut channel, typed).await);
        }
        seen.push(' ');
        seen.push_str(&full(&mut channel).await);
        assert!(seen.contains("Waiting room"), "{seen}");
        for leak in [
            "bob",
            "Bobs Secret Name",
            "pending_pat",
            "Members",
            "Member list",
            "Profile",
        ] {
            assert!(!seen.contains(leak), "{leak} reached a non-member: {seen}");
        }
    }
    rig.server.shutdown().await;
}

#[tokio::test]
async fn losing_members_view_closes_the_screen_on_the_next_key() {
    let rig = Rig::start().await;
    let (_, key) = rig.member("alice").await;
    rig.member("bob").await;
    let (_session, mut channel) = at_menu(&rig, &key).await;
    send(&mut channel, "ul").await;
    assert!(full(&mut channel).await.contains("2. bob"));

    let taken = rig
        .revoke(
            Permission::MembersView,
            &["member", "trusted", "moderator", "cosysop", "sysop"],
        )
        .await;
    send(&mut channel, "2").await;
    let screen = full(&mut channel).await;
    assert!(
        screen.contains("You are not allowed to do that."),
        "{screen}"
    );
    assert!(
        !screen.contains("bob") && !screen.contains("Page 1"),
        "{screen}"
    );
    assert!(
        !screen.contains("[l] Member list"),
        "the item is gone: {screen}"
    );
    // The hotkey of a hidden item does nothing.
    send(&mut channel, "l").await;
    let still = full(&mut channel).await;
    assert!(
        !still.contains("Page 1") && !still.contains("1. alice"),
        "{still}"
    );

    rig.restore(&taken, Permission::MembersView).await;
    send(&mut channel, "x").await;
    assert!(full(&mut channel).await.contains("[l] Member list"));
    send(&mut channel, "l").await;
    assert!(full(&mut channel).await.contains("1. alice"));
    rig.server.shutdown().await;
}
