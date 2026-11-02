//! Account, fingerprint, settings and membership behaviour, against any backend.
#![allow(clippy::unwrap_used)]

use russh_bbs_core::access::{AccessStore, MemberDirectory, Slug};
use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::store::{
    Account, AccountState, AccountStore, NewAccount, RegistrationMode, SettingKey, SettingsStore,
    StoreError, registration_mode, set_registration_mode,
};

use super::{Harness, fingerprint};

fn new_account(name: &str, state: AccountState) -> NewAccount {
    NewAccount {
        username: sanitize_username(name).unwrap(),
        role: Slug::MEMBER,
        state,
    }
}

async fn account<S: AccountStore>(store: &S, name: &str, state: AccountState) -> Account {
    store
        .create_account(new_account(name, state))
        .await
        .unwrap()
}

pub(crate) async fn creates_an_account_with_its_fields<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let created = store
        .create_account(NewAccount {
            username: sanitize_username("alice").unwrap(),
            role: Slug::SYSOP,
            state: AccountState::Active,
        })
        .await
        .unwrap();
    assert_eq!(created.username.as_str(), "alice");
    // Authority is the sysop grant, not the legacy `role` column.
    let snapshot = store.snapshot(Some(created.id)).await.unwrap();
    assert_eq!(snapshot.top_role(), Some(&Slug::SYSOP));
    assert_eq!(created.state, AccountState::Active);
    assert!(created.created_at.as_raw() > 0);
}

pub(crate) async fn a_known_fingerprint_returns_its_account_and_an_unknown_one_returns_none<
    H: Harness,
>() {
    let h = H::fresh().await;
    let store = h.store();
    let alice = account(store, "alice", AccountState::Active).await;
    store
        .attach_fingerprint(alice.id, &fingerprint(1))
        .await
        .unwrap();

    let found = store.account_by_fingerprint(&fingerprint(1)).await.unwrap();
    assert_eq!(found, Some(alice));
    let unknown = store.account_by_fingerprint(&fingerprint(2)).await.unwrap();
    assert_eq!(unknown, None);
}

pub(crate) async fn an_account_can_have_several_keys<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let alice = account(store, "alice", AccountState::Active).await;
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

pub(crate) async fn two_accounts_cannot_share_a_fingerprint<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let alice = account(store, "alice", AccountState::Active).await;
    let bob = account(store, "bob", AccountState::Active).await;
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

pub(crate) async fn attaching_to_a_missing_account_is_an_error<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let ghost = russh_bbs_core::store::AccountId::from_raw(9999);
    let result = store.attach_fingerprint(ghost, &fingerprint(1)).await;
    assert!(
        matches!(result, Err(StoreError::AccountNotFound)),
        "{result:?}"
    );
}

pub(crate) async fn usernames_are_unique_including_lookalikes<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    account(store, "alice", AccountState::Active).await;
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

pub(crate) async fn registration_mode_defaults_to_invite_and_can_change<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    assert_eq!(
        store
            .get_setting(SettingKey::RegistrationMode)
            .await
            .unwrap()
            .as_deref(),
        Some("invite")
    );
    assert_eq!(
        registration_mode(store).await.unwrap(),
        RegistrationMode::Invite
    );

    set_registration_mode(store, RegistrationMode::Open)
        .await
        .unwrap();
    assert_eq!(
        registration_mode(store).await.unwrap(),
        RegistrationMode::Open
    );
}

pub(crate) async fn settings_get_set_and_overwrite<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
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

pub(crate) async fn an_unrecognised_registration_mode_is_an_error_not_a_default<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    store
        .set_setting(SettingKey::RegistrationMode, "wide-open")
        .await
        .unwrap();
    let result = registration_mode(store).await;
    assert!(matches!(result, Err(StoreError::Corrupt(_))), "{result:?}");
}

pub(crate) async fn only_active_accounts_are_members<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let states = [
        (1, "active_one", AccountState::Active, true),
        (2, "pending_one", AccountState::Pending, false),
        (3, "approved_one", AccountState::Approved, false),
        (4, "suspended_one", AccountState::Suspended, false),
    ];
    for (n, name, state, _) in states {
        let created = account(store, name, state).await;
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

pub(crate) async fn the_directory_fails_closed_when_the_store_is_broken<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let alice = account(store, "alice", AccountState::Active).await;
    store
        .attach_fingerprint(alice.id, &fingerprint(1))
        .await
        .unwrap();
    assert!(store.is_member(&fingerprint(1)).await);

    h.close().await;
    assert!(!store.is_member(&fingerprint(1)).await);
}

pub(crate) async fn data_survives_a_reopen<H: Harness>() {
    let mut h = H::fresh().await;
    h.store()
        .set_setting(SettingKey::FounderClaimed, "true")
        .await
        .unwrap();
    h.reopen().await;
    let value = h
        .store()
        .get_setting(SettingKey::FounderClaimed)
        .await
        .unwrap();
    assert_eq!(value.as_deref(), Some("true"));
}
