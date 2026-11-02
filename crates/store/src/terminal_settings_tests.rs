//! Terminal settings and login times.
//!
//! The behaviour lives in `suite`, a set of checks written against the core traits only
//! (`AccountStore`, `TerminalSettingsStore`, `MemberSessions`), so M1-G can run the same suite
//! against Postgres. The tests at the bottom run it on SQLite and add the checks that need the
//! SQLite file (corrupt rows, cascades).
#![allow(clippy::unwrap_used)]

use russh_bbs_core::icons::Tier;
use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::store::{
    Account, AccountId, AccountState, AccountStore, NewAccount, Role, StoreError,
};
use russh_bbs_core::terminal_settings::{MemberSessions, TerminalSettings, TerminalSettingsStore};

use crate::SqliteStore;
use crate::tests::{fingerprint, temp_store};

async fn account<S: AccountStore>(store: &S, name: &str, state: AccountState) -> Account {
    store
        .create_account(NewAccount {
            username: sanitize_username(name).unwrap(),
            role: Role::Member,
            state,
        })
        .await
        .unwrap()
}

/// The checks every backend must pass.
mod suite {
    use super::*;

    pub(super) async fn defaults_when_missing<S: AccountStore + TerminalSettingsStore>(store: &S) {
        let alice = account(store, "alice", AccountState::Active).await;
        let settings = store.terminal_settings(alice.id).await.unwrap();
        assert_eq!(settings, TerminalSettings::default());
        assert!(settings.needs_glyph_test());
        assert!(settings.pause());
    }

    pub(super) async fn set_then_get_round_trips<S: AccountStore + TerminalSettingsStore>(
        store: &S,
    ) {
        let alice = account(store, "alice", AccountState::Active).await;
        let wanted = TerminalSettings::default()
            .with_width(Some(120))
            .unwrap()
            .with_height(Some(40))
            .unwrap()
            .with_color(false)
            .with_pause(false)
            .with_tier(Tier::Unicode);
        store
            .set_terminal_settings(alice.id, &wanted)
            .await
            .unwrap();
        assert_eq!(store.terminal_settings(alice.id).await.unwrap(), wanted);
    }

    pub(super) async fn every_tier_and_unset_round_trip<S: AccountStore + TerminalSettingsStore>(
        store: &S,
    ) {
        let alice = account(store, "alice", AccountState::Active).await;
        for tier in Tier::ALL {
            let wanted = TerminalSettings::default().with_tier(tier);
            store
                .set_terminal_settings(alice.id, &wanted)
                .await
                .unwrap();
            assert_eq!(store.terminal_settings(alice.id).await.unwrap(), wanted);
        }
        let unset = TerminalSettings::default();
        store.set_terminal_settings(alice.id, &unset).await.unwrap();
        assert_eq!(store.terminal_settings(alice.id).await.unwrap(), unset);
    }

    pub(super) async fn a_later_set_replaces_and_clears_overrides<
        S: AccountStore + TerminalSettingsStore,
    >(
        store: &S,
    ) {
        let alice = account(store, "alice", AccountState::Active).await;
        let sized = TerminalSettings::default().with_width(Some(100)).unwrap();
        store.set_terminal_settings(alice.id, &sized).await.unwrap();
        let cleared = sized.with_width(None).unwrap().with_tier(Tier::Ascii);
        store
            .set_terminal_settings(alice.id, &cleared)
            .await
            .unwrap();
        let read = store.terminal_settings(alice.id).await.unwrap();
        assert_eq!(read, cleared);
        assert_eq!(read.width(), None);
    }

    pub(super) async fn accounts_are_isolated<S: AccountStore + TerminalSettingsStore>(store: &S) {
        let alice = account(store, "alice", AccountState::Active).await;
        let bob = account(store, "bobby", AccountState::Active).await;
        let a = TerminalSettings::default().with_tier(Tier::NerdFont);
        store.set_terminal_settings(alice.id, &a).await.unwrap();
        assert_eq!(
            store.terminal_settings(bob.id).await.unwrap(),
            TerminalSettings::default()
        );
        let b = TerminalSettings::default()
            .safe_fallback()
            .with_pause(false);
        store.set_terminal_settings(bob.id, &b).await.unwrap();
        assert_eq!(store.terminal_settings(alice.id).await.unwrap(), a);
        assert_eq!(store.terminal_settings(bob.id).await.unwrap(), b);
    }

    pub(super) async fn saving_for_an_unknown_account_fails<S: TerminalSettingsStore>(store: &S) {
        let result = store
            .set_terminal_settings(AccountId::from_raw(9_999), &TerminalSettings::default())
            .await;
        assert!(
            matches!(result, Err(StoreError::AccountNotFound)),
            "{result:?}"
        );
        let result = store.record_login(AccountId::from_raw(9_999)).await;
        assert!(
            matches!(result, Err(StoreError::AccountNotFound)),
            "{result:?}"
        );
    }

    pub(super) async fn login_times_report_the_previous_login<
        S: AccountStore + TerminalSettingsStore,
    >(
        store: &S,
    ) {
        let alice = account(store, "alice", AccountState::Active).await;
        let bob = account(store, "bobby", AccountState::Active).await;
        assert_eq!(
            store.record_login(alice.id).await.unwrap(),
            None,
            "first login"
        );
        let second = store.record_login(alice.id).await.unwrap();
        assert!(second.is_some_and(|t| t.as_raw() > 0), "{second:?}");
        assert_eq!(
            store.record_login(bob.id).await.unwrap(),
            None,
            "bob's own first"
        );
    }

    pub(super) async fn sessions_start_from_stored_state<S: MemberSessions + AccountStore>(
        store: &S,
    ) {
        let alice = account(store, "alice", AccountState::Active).await;
        let key = fingerprint(1);
        store.attach_fingerprint(alice.id, &key).await.unwrap();

        let first = store.start_for_key(&key).await.unwrap().unwrap();
        assert_eq!(first.account, alice.id);
        assert_eq!(first.handle.as_str(), "alice");
        assert_eq!(first.role, Role::Member);
        assert_eq!(first.settings, TerminalSettings::default());
        assert_eq!(first.previous_login, None);

        let chosen = first.settings.with_tier(Tier::Unicode);
        store.save_settings(alice.id, &chosen).await.unwrap();
        let second = store.start_for_key(&key).await.unwrap().unwrap();
        assert_eq!(second.settings, chosen, "the stored tier is used");
        assert!(second.previous_login.is_some());
    }

    pub(super) async fn only_active_accounts_start_a_session<S: MemberSessions + AccountStore>(
        store: &S,
    ) {
        assert!(
            store
                .start_for_key(&fingerprint(7))
                .await
                .unwrap()
                .is_none(),
            "unknown key"
        );
        let pending = account(store, "pendy", AccountState::Pending).await;
        let key = fingerprint(2);
        store.attach_fingerprint(pending.id, &key).await.unwrap();
        assert!(
            store.start_for_key(&key).await.unwrap().is_none(),
            "pending account"
        );
    }
}

#[tokio::test]
async fn sqlite_defaults_when_missing() {
    let (_dir, store) = temp_store().await;
    suite::defaults_when_missing(&store).await;
}

#[tokio::test]
async fn sqlite_set_then_get_round_trips() {
    let (_dir, store) = temp_store().await;
    suite::set_then_get_round_trips(&store).await;
}

#[tokio::test]
async fn sqlite_every_tier_and_unset_round_trip() {
    let (_dir, store) = temp_store().await;
    suite::every_tier_and_unset_round_trip(&store).await;
}

#[tokio::test]
async fn sqlite_a_later_set_replaces_and_clears_overrides() {
    let (_dir, store) = temp_store().await;
    suite::a_later_set_replaces_and_clears_overrides(&store).await;
}

#[tokio::test]
async fn sqlite_accounts_are_isolated() {
    let (_dir, store) = temp_store().await;
    suite::accounts_are_isolated(&store).await;
}

#[tokio::test]
async fn sqlite_saving_for_an_unknown_account_fails() {
    let (_dir, store) = temp_store().await;
    suite::saving_for_an_unknown_account_fails(&store).await;
}

#[tokio::test]
async fn sqlite_login_times_report_the_previous_login() {
    let (_dir, store) = temp_store().await;
    suite::login_times_report_the_previous_login(&store).await;
}

#[tokio::test]
async fn sqlite_sessions_start_from_stored_state() {
    let (_dir, store) = temp_store().await;
    suite::sessions_start_from_stored_state(&store).await;
}

#[tokio::test]
async fn sqlite_only_active_accounts_start_a_session() {
    let (_dir, store) = temp_store().await;
    suite::only_active_accounts_start_a_session(&store).await;
}

// ---- SQLite-specific ----

#[tokio::test]
async fn settings_survive_reopening_the_database() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bbs.db");
    let store = SqliteStore::open(&path).await.unwrap();
    let alice = account(&store, "alice", AccountState::Active).await;
    let wanted = TerminalSettings::default().with_tier(Tier::NerdFont);
    store
        .set_terminal_settings(alice.id, &wanted)
        .await
        .unwrap();
    store.close().await;

    let store = SqliteStore::open(&path).await.unwrap();
    assert_eq!(store.terminal_settings(alice.id).await.unwrap(), wanted);
}

#[tokio::test]
async fn a_row_the_code_could_not_have_written_is_corrupt_not_a_default() {
    let (_dir, store) = temp_store().await;
    let alice = account(&store, "alice", AccountState::Active).await;
    // The CHECK constraints stop most bad values, so write the ones they allow but core rejects.
    sqlx::query(
        "INSERT INTO terminal_settings (account_id, width_override, color, pause) VALUES (?, 5, 1, 1)",
    )
    .bind(alice.id.as_raw())
    .execute(store.pool())
    .await
    .unwrap();
    let result = store.terminal_settings(alice.id).await;
    assert!(matches!(result, Err(StoreError::Corrupt(_))), "{result:?}");
}

#[tokio::test]
async fn the_database_refuses_an_unknown_tier_or_flag() {
    let (_dir, store) = temp_store().await;
    let alice = account(&store, "alice", AccountState::Active).await;
    for (color, tier) in [(2, "ascii"), (1, "emoji")] {
        let result = sqlx::query(
            "INSERT INTO terminal_settings (account_id, color, pause, icon_tier) VALUES (?, ?, 1, ?)",
        )
        .bind(alice.id.as_raw())
        .bind(color)
        .bind(tier)
        .execute(store.pool())
        .await;
        assert!(result.is_err(), "color={color} tier={tier}");
    }
}

#[tokio::test]
async fn deleting_an_account_deletes_its_settings_and_logins() {
    let (_dir, store) = temp_store().await;
    let alice = account(&store, "alice", AccountState::Active).await;
    store
        .set_terminal_settings(alice.id, &TerminalSettings::default().safe_fallback())
        .await
        .unwrap();
    store.record_login(alice.id).await.unwrap();
    sqlx::query("DELETE FROM accounts WHERE id = ?")
        .bind(alice.id.as_raw())
        .execute(store.pool())
        .await
        .unwrap();
    for table in ["terminal_settings", "account_logins"] {
        let n: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert_eq!(n, 0, "{table}");
    }
}

#[tokio::test]
async fn concurrent_logins_each_see_a_consistent_previous_time() {
    let (_dir, store) = temp_store().await;
    let alice = account(&store, "alice", AccountState::Active).await;
    store.record_login(alice.id).await.unwrap();
    let tasks: Vec<_> = (0..8)
        .map(|_| {
            let store = store.clone();
            tokio::spawn(async move { store.record_login(alice.id).await })
        })
        .collect();
    for task in tasks {
        let previous = task.await.unwrap().unwrap();
        assert!(previous.is_some(), "every later login has a previous one");
    }
}
