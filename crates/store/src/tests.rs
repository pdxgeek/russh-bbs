//! Store behaviour against a real temporary SQLite file with the real migrations.
#![allow(clippy::unwrap_used)]

use russh_bbs_core::access::{Fingerprint, MemberDirectory};
use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::store::{
    Account, AccountState, AccountStore, NewAccount, RegistrationMode, Role, SettingKey,
    SettingsStore, StoreError, registration_mode, set_registration_mode,
};
use tempfile::TempDir;

use crate::SqliteStore;

/// A valid-shaped fingerprint that differs per `n`.
pub(crate) fn fingerprint(n: u32) -> Fingerprint {
    Fingerprint::parse(&format!("SHA256:{n:A>43}")).unwrap()
}

pub(crate) async fn temp_store() -> (TempDir, SqliteStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::open(&dir.path().join("bbs.db")).await.unwrap();
    (dir, store)
}

fn new_account(name: &str, state: AccountState) -> NewAccount {
    NewAccount {
        username: sanitize_username(name).unwrap(),
        role: Role::Member,
        state,
    }
}

async fn account(store: &SqliteStore, name: &str, state: AccountState) -> Account {
    store
        .create_account(new_account(name, state))
        .await
        .unwrap()
}

#[tokio::test]
async fn creates_an_account_with_its_fields() {
    let (_dir, store) = temp_store().await;
    let created = store
        .create_account(NewAccount {
            username: sanitize_username("alice").unwrap(),
            role: Role::Sysop,
            state: AccountState::Active,
        })
        .await
        .unwrap();
    assert_eq!(created.username.as_str(), "alice");
    assert_eq!(created.role, Role::Sysop);
    assert_eq!(created.state, AccountState::Active);
    assert!(created.created_at.as_raw() > 0);
}

#[tokio::test]
async fn a_known_fingerprint_returns_its_account_and_an_unknown_one_returns_none() {
    let (_dir, store) = temp_store().await;
    let alice = account(&store, "alice", AccountState::Active).await;
    store
        .attach_fingerprint(alice.id, &fingerprint(1))
        .await
        .unwrap();

    let found = store.account_by_fingerprint(&fingerprint(1)).await.unwrap();
    assert_eq!(found, Some(alice));
    let unknown = store.account_by_fingerprint(&fingerprint(2)).await.unwrap();
    assert_eq!(unknown, None);
}

#[tokio::test]
async fn an_account_can_have_several_keys() {
    let (_dir, store) = temp_store().await;
    let alice = account(&store, "alice", AccountState::Active).await;
    for n in [1, 2] {
        store
            .attach_fingerprint(alice.id, &fingerprint(n))
            .await
            .unwrap();
    }
    for n in [1, 2] {
        let found = store.account_by_fingerprint(&fingerprint(n)).await.unwrap();
        assert_eq!(found.map(|a| a.id), Some(alice.id));
    }
}

#[tokio::test]
async fn two_accounts_cannot_share_a_fingerprint() {
    let (_dir, store) = temp_store().await;
    let alice = account(&store, "alice", AccountState::Active).await;
    let bob = account(&store, "bob", AccountState::Active).await;
    store
        .attach_fingerprint(alice.id, &fingerprint(1))
        .await
        .unwrap();

    let other = store.attach_fingerprint(bob.id, &fingerprint(1)).await;
    assert!(
        matches!(other, Err(StoreError::FingerprintTaken)),
        "{other:?}"
    );
    let same = store.attach_fingerprint(alice.id, &fingerprint(1)).await;
    assert!(
        matches!(same, Err(StoreError::FingerprintTaken)),
        "{same:?}"
    );
    // The key still belongs to the first account.
    let found = store.account_by_fingerprint(&fingerprint(1)).await.unwrap();
    assert_eq!(found.map(|a| a.id), Some(alice.id));
}

#[tokio::test]
async fn attaching_to_a_missing_account_is_an_error() {
    let (_dir, store) = temp_store().await;
    let ghost = russh_bbs_core::store::AccountId::from_raw(9999);
    let result = store.attach_fingerprint(ghost, &fingerprint(1)).await;
    assert!(
        matches!(result, Err(StoreError::AccountNotFound)),
        "{result:?}"
    );
}

#[tokio::test]
async fn usernames_are_unique_including_lookalikes() {
    let (_dir, store) = temp_store().await;
    account(&store, "alice", AccountState::Active).await;
    let same = store
        .create_account(new_account("alice", AccountState::Active))
        .await;
    assert!(matches!(same, Err(StoreError::UsernameTaken)), "{same:?}");
    // `A1ice` skeletons to the same key as `alice` (D17).
    let lookalike = store
        .create_account(new_account("A1ice", AccountState::Active))
        .await;
    assert!(
        matches!(lookalike, Err(StoreError::UsernameTaken)),
        "{lookalike:?}"
    );
}

#[tokio::test]
async fn registration_mode_defaults_to_invite_and_can_change() {
    let (_dir, store) = temp_store().await;
    assert_eq!(
        store
            .get_setting(SettingKey::RegistrationMode)
            .await
            .unwrap()
            .as_deref(),
        Some("invite")
    );
    assert_eq!(
        registration_mode(&store).await.unwrap(),
        RegistrationMode::Invite
    );

    set_registration_mode(&store, RegistrationMode::Open)
        .await
        .unwrap();
    assert_eq!(
        registration_mode(&store).await.unwrap(),
        RegistrationMode::Open
    );
}

#[tokio::test]
async fn settings_get_set_and_overwrite() {
    let (_dir, store) = temp_store().await;
    assert_eq!(
        store.get_setting(SettingKey::FounderClaimed).await.unwrap(),
        None
    );
    store
        .set_setting(SettingKey::FounderClaimed, "true")
        .await
        .unwrap();
    store
        .set_setting(SettingKey::FounderClaimed, "false")
        .await
        .unwrap();
    assert_eq!(
        store
            .get_setting(SettingKey::FounderClaimed)
            .await
            .unwrap()
            .as_deref(),
        Some("false")
    );
}

#[tokio::test]
async fn an_unrecognised_registration_mode_is_an_error_not_a_default() {
    let (_dir, store) = temp_store().await;
    store
        .set_setting(SettingKey::RegistrationMode, "wide-open")
        .await
        .unwrap();
    let result = registration_mode(&store).await;
    assert!(matches!(result, Err(StoreError::Corrupt(_))), "{result:?}");
}

#[tokio::test]
async fn only_active_accounts_are_members() {
    let (_dir, store) = temp_store().await;
    let states = [
        (1, "active_one", AccountState::Active, true),
        (2, "pending_one", AccountState::Pending, false),
        (3, "approved_one", AccountState::Approved, false),
        (4, "suspended_one", AccountState::Suspended, false),
    ];
    for (n, name, state, _) in states {
        let created = account(&store, name, state).await;
        store
            .attach_fingerprint(created.id, &fingerprint(n))
            .await
            .unwrap();
    }
    for (n, name, _, expected) in states {
        assert_eq!(store.is_member(&fingerprint(n)).await, expected, "{name}");
    }
    assert!(!store.is_member(&fingerprint(99)).await, "unknown key");
}

#[tokio::test]
async fn the_directory_fails_closed_when_the_store_is_broken() {
    let (_dir, store) = temp_store().await;
    let alice = account(&store, "alice", AccountState::Active).await;
    store
        .attach_fingerprint(alice.id, &fingerprint(1))
        .await
        .unwrap();
    assert!(store.is_member(&fingerprint(1)).await);

    store.close().await;
    assert!(!store.is_member(&fingerprint(1)).await);
}

#[tokio::test]
async fn corrupt_stored_values_are_reported_not_trusted() {
    let (_dir, store) = temp_store().await;
    let alice = account(&store, "alice", AccountState::Active).await;
    store
        .attach_fingerprint(alice.id, &fingerprint(1))
        .await
        .unwrap();
    // Bypass the CHECK constraint the way a future/buggy writer might: rename a username to
    // something the sanitizer rejects.
    sqlx::query("UPDATE accounts SET username = 'bad name'")
        .execute(store.pool())
        .await
        .unwrap();
    let result = store.account_by_fingerprint(&fingerprint(1)).await;
    assert!(matches!(result, Err(StoreError::Corrupt(_))), "{result:?}");
    assert!(
        !store.is_member(&fingerprint(1)).await,
        "corrupt row must fail closed"
    );
}
