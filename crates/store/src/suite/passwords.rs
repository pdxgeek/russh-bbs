//! Password-hash storage behaviour, against any backend (M3-D, D81).
#![allow(clippy::unwrap_used, clippy::assert_is_empty)]

use russh_bbs_core::access::Slug;
use russh_bbs_core::audit::{Action, AuditQuery, AuditStore, LogEntry, events};
use russh_bbs_core::passwords::PasswordStore;
use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::store::{
    Account, AccountId, AccountState, AccountStore, NewAccount, StoreError,
};

use super::Harness;

const HASH_A: &str =
    "$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHRzYWx0$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const HASH_B: &str =
    "$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHRzYWx0$BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB";

async fn account<S: AccountStore>(store: &S, name: &str, state: AccountState) -> Account {
    store
        .create_account(NewAccount {
            username: sanitize_username(name).unwrap(),
            role: Slug::MEMBER,
            state,
        })
        .await
        .unwrap()
}

async fn audit_actions<S: AuditStore>(store: &S) -> Vec<Action> {
    let mut rows: Vec<Action> = store
        .page(&AuditQuery::default())
        .await
        .unwrap()
        .into_iter()
        .filter_map(|entry| match entry {
            LogEntry::Event(row) => Some(row.event.action),
            LogEntry::Unknown(_) => None,
        })
        .filter(|action| matches!(action, Action::PasswordSet | Action::PasswordCleared))
        .collect();
    rows.reverse();
    rows
}

pub(crate) async fn a_new_account_has_no_password<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let alice = account(store, "alice", AccountState::Active).await;
    assert_eq!(store.stored_hash(alice.id).await.unwrap(), None);
    let record = store.login_record("alice").await.unwrap().unwrap();
    assert_eq!(record.account.id, alice.id);
    assert_eq!(record.hash, None);
}

pub(crate) async fn a_hash_round_trips_and_a_later_one_replaces_it<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let alice = account(store, "alice", AccountState::Active).await;
    let bob = account(store, "bob", AccountState::Active).await;
    store
        .set_hash(
            alice.id,
            Some(HASH_A),
            &events::password_set(alice.id, "key session"),
        )
        .await
        .unwrap();
    assert_eq!(
        store.stored_hash(alice.id).await.unwrap().as_deref(),
        Some(HASH_A)
    );
    assert_eq!(
        store.stored_hash(bob.id).await.unwrap(),
        None,
        "another account is untouched"
    );
    store
        .set_hash(
            alice.id,
            Some(HASH_B),
            &events::password_set(alice.id, "key session"),
        )
        .await
        .unwrap();
    assert_eq!(
        store.stored_hash(alice.id).await.unwrap().as_deref(),
        Some(HASH_B)
    );
    let record = store.login_record("alice").await.unwrap().unwrap();
    assert_eq!(record.hash.as_deref(), Some(HASH_B));
}

pub(crate) async fn the_login_lookup_ignores_case_and_finds_every_state<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    account(store, "Alice", AccountState::Active).await;
    let locked = account(store, "locked", AccountState::Suspended).await;
    assert!(store.login_record("alice").await.unwrap().is_some());
    assert!(store.login_record("nobody").await.unwrap().is_none());
    // Core decides what a state may do; the store reports it.
    let found = store.login_record("locked").await.unwrap().unwrap();
    assert_eq!(found.account.id, locked.id);
    assert_eq!(found.account.state, AccountState::Suspended);
    // A pattern character is a literal, not a wildcard.
    assert!(store.login_record("%").await.unwrap().is_none());
    assert!(store.login_record("al_ce").await.unwrap().is_none());
}

pub(crate) async fn clearing_removes_the_hash_and_audits_only_a_real_change<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let alice = account(store, "alice", AccountState::Active).await;
    // Nothing to clear: no change and no audit row.
    store
        .set_hash(
            alice.id,
            None,
            &events::password_cleared(alice.id, "key session"),
        )
        .await
        .unwrap();
    assert!(audit_actions(store).await.is_empty());
    store
        .set_hash(
            alice.id,
            Some(HASH_A),
            &events::password_set(alice.id, "key session"),
        )
        .await
        .unwrap();
    store
        .set_hash(
            alice.id,
            None,
            &events::password_cleared(alice.id, "key session"),
        )
        .await
        .unwrap();
    assert_eq!(store.stored_hash(alice.id).await.unwrap(), None);
    assert_eq!(
        audit_actions(store).await,
        [Action::PasswordSet, Action::PasswordCleared]
    );
}

pub(crate) async fn the_audit_row_holds_no_hash<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let alice = account(store, "alice", AccountState::Active).await;
    store
        .set_hash(
            alice.id,
            Some(HASH_A),
            &events::password_set(alice.id, "password session"),
        )
        .await
        .unwrap();
    let rows = h
        .texts("SELECT detail || ' ' || target_ref || ' ' || action FROM audit_log WHERE action LIKE 'password.%'")
        .await;
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].contains("argon2"));
    assert!(!rows[0].contains(HASH_A));
    assert!(rows[0].contains("password.set"));
}

pub(crate) async fn an_unknown_account_is_not_found_on_every_call<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let ghost = AccountId::from_raw(4242);
    assert!(matches!(
        store.stored_hash(ghost).await,
        Err(StoreError::AccountNotFound)
    ));
    assert!(matches!(
        store
            .set_hash(
                ghost,
                Some(HASH_A),
                &events::password_set(ghost, "key session")
            )
            .await,
        Err(StoreError::AccountNotFound)
    ));
    assert!(matches!(
        store
            .set_hash(ghost, None, &events::password_cleared(ghost, "key session"))
            .await,
        Err(StoreError::AccountNotFound)
    ));
    assert!(audit_actions(store).await.is_empty());
}

pub(crate) async fn an_upgrade_replaces_only_the_hash_it_was_read_from<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let alice = account(store, "alice", AccountState::Active).await;
    store
        .set_hash(
            alice.id,
            Some(HASH_A),
            &events::password_set(alice.id, "key session"),
        )
        .await
        .unwrap();
    // A password change happened meanwhile: the slower upgrade must not undo it.
    assert!(
        !store
            .upgrade_hash(alice.id, HASH_B, "$argon2id$stale")
            .await
            .unwrap()
    );
    assert_eq!(
        store.stored_hash(alice.id).await.unwrap().as_deref(),
        Some(HASH_A)
    );
    assert!(store.upgrade_hash(alice.id, HASH_A, HASH_B).await.unwrap());
    assert_eq!(
        store.stored_hash(alice.id).await.unwrap().as_deref(),
        Some(HASH_B)
    );
    // An upgrade writes no audit row: the password did not change (only the first set did).
    assert_eq!(audit_actions(store).await, [Action::PasswordSet]);
    // Nothing to upgrade for an account with no password.
    let bob = account(store, "bob", AccountState::Active).await;
    assert!(!store.upgrade_hash(bob.id, HASH_A, HASH_B).await.unwrap());
    assert_eq!(store.stored_hash(bob.id).await.unwrap(), None);
}

pub(crate) async fn a_failed_audit_write_leaves_the_password_unchanged<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let alice = account(store, "alice", AccountState::Active).await;
    store
        .set_hash(
            alice.id,
            Some(HASH_A),
            &events::password_set(alice.id, "key session"),
        )
        .await
        .unwrap();
    h.break_audit_inserts().await;
    assert!(
        store
            .set_hash(
                alice.id,
                Some(HASH_B),
                &events::password_set(alice.id, "key session")
            )
            .await
            .is_err()
    );
    assert!(
        store
            .set_hash(
                alice.id,
                None,
                &events::password_cleared(alice.id, "key session")
            )
            .await
            .is_err()
    );
    assert_eq!(
        store.stored_hash(alice.id).await.unwrap().as_deref(),
        Some(HASH_A)
    );
}

pub(crate) async fn the_database_refuses_anything_but_an_argon2id_string<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let alice = account(store, "alice", AccountState::Active).await;
    for bad in ["", "plaintext-password", "$2b$12$abcdefghijklmnopqrstuv"] {
        assert!(
            store
                .set_hash(
                    alice.id,
                    Some(bad),
                    &events::password_set(alice.id, "key session")
                )
                .await
                .is_err(),
            "{bad:?} must be refused"
        );
    }
    assert_eq!(store.stored_hash(alice.id).await.unwrap(), None);
}

pub(crate) async fn a_hash_survives_a_reopen_and_goes_with_its_account<H: Harness>() {
    let mut h = H::fresh().await;
    let alice = account(h.store(), "alice", AccountState::Active).await;
    h.store()
        .set_hash(
            alice.id,
            Some(HASH_A),
            &events::password_set(alice.id, "key session"),
        )
        .await
        .unwrap();
    h.reopen().await;
    assert_eq!(
        h.store().stored_hash(alice.id).await.unwrap().as_deref(),
        Some(HASH_A)
    );
    // The row belongs to the account: deleting the account removes it.
    h.raw(&format!(
        "DELETE FROM accounts WHERE id = {}",
        alice.id.as_raw()
    ))
    .await
    .unwrap();
    let left = h
        .texts("SELECT CAST(count(*) AS TEXT) FROM account_password")
        .await;
    assert_eq!(left, ["0"]);
}
