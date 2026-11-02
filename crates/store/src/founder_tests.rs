//! The founder claim against a real SQLite file: atomicity, the race, and rollback.
#![allow(clippy::unwrap_used)]

use russh_bbs_core::access::{MemberDirectory, RoleDirectory};
use russh_bbs_core::founder::FounderStore;
use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::store::{
    AccountState, AccountStore, NewAccount, Role, SettingKey, SettingsStore, StoreError,
};

use crate::SqliteStore;
use crate::tests::{fingerprint, temp_store};

fn name() -> russh_bbs_core::sanitize::Username {
    sanitize_username(russh_bbs_core::founder::FOUNDER_USERNAME).unwrap()
}

#[tokio::test]
async fn a_claim_creates_an_active_sysop_bound_to_the_key() {
    let (_dir, store) = temp_store().await;
    assert!(!store.founder_claimed().await.unwrap());
    let key = fingerprint(1);
    let account = store.claim_founder(name(), &key).await.unwrap().unwrap();
    assert_eq!(account.role, Role::Sysop);
    assert_eq!(account.state, AccountState::Active);
    assert!(store.founder_claimed().await.unwrap());
    assert!(store.is_member(&key).await);
    assert_eq!(store.role_of(&key).await, Some(Role::Sysop));
    assert_eq!(
        store
            .get_setting(SettingKey::FounderClaimed)
            .await
            .unwrap()
            .as_deref(),
        Some("true")
    );
}

#[tokio::test]
async fn a_second_claim_changes_nothing() {
    let (_dir, store) = temp_store().await;
    store
        .claim_founder(name(), &fingerprint(1))
        .await
        .unwrap()
        .unwrap();
    let again = store
        .claim_founder(sanitize_username("other").unwrap(), &fingerprint(2))
        .await
        .unwrap();
    assert!(again.is_none());
    assert!(!store.is_member(&fingerprint(2)).await);
    assert!(
        store
            .account_by_fingerprint(&fingerprint(2))
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_concurrent_claims_produce_exactly_one_sysop() {
    for round in 0..20_u32 {
        let (_dir, store) = temp_store().await;
        let (a, b) = {
            let (s1, s2) = (store.clone(), store.clone());
            let k1 = fingerprint(100 + round);
            let k2 = fingerprint(200 + round);
            tokio::join!(
                tokio::spawn(async move { s1.claim_founder(name(), &k1).await }),
                tokio::spawn(async move { s2.claim_founder(name(), &k2).await }),
            )
        };
        let results = [a.unwrap(), b.unwrap()];
        let wins = results.iter().filter(|r| matches!(r, Ok(Some(_)))).count();
        assert_eq!(wins, 1, "round {round}: {results:?}");
        // The loser either saw the flag (Ok(None)) or lost on the name; never a second sysop.
        let members = [fingerprint(100 + round), fingerprint(200 + round)];
        let mut sysops = 0;
        for key in &members {
            if store.role_of(key).await == Some(Role::Sysop) {
                sysops += 1;
            }
        }
        assert_eq!(sysops, 1, "round {round}");
    }
}

#[tokio::test]
async fn a_failed_claim_rolls_back_and_leaves_the_board_unclaimed() {
    let (_dir, store) = temp_store().await;
    // The key is already bound to an ordinary member, so the claim must fail on the key and
    // must not leave the flag or a half-made account behind.
    let member = store
        .create_account(NewAccount {
            username: sanitize_username("alice").unwrap(),
            role: Role::Member,
            state: AccountState::Active,
        })
        .await
        .unwrap();
    store
        .attach_fingerprint(member.id, &fingerprint(1))
        .await
        .unwrap();

    let err = store
        .claim_founder(name(), &fingerprint(1))
        .await
        .unwrap_err();
    assert!(matches!(err, StoreError::FingerprintTaken), "{err:?}");
    assert!(!store.founder_claimed().await.unwrap());
    // The sysop name is still free, and a later claim by another key works.
    let account = store.claim_founder(name(), &fingerprint(2)).await.unwrap();
    assert!(account.is_some());
}

#[tokio::test]
async fn the_claim_survives_reopening_the_database() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bbs.db");
    let store = SqliteStore::open(&path).await.unwrap();
    store
        .claim_founder(name(), &fingerprint(1))
        .await
        .unwrap()
        .unwrap();
    store.close().await;
    let reopened = SqliteStore::open(&path).await.unwrap();
    assert!(reopened.founder_claimed().await.unwrap());
    assert_eq!(reopened.role_of(&fingerprint(1)).await, Some(Role::Sysop));
    assert!(
        reopened
            .claim_founder(name(), &fingerprint(2))
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn role_lookup_is_only_for_active_accounts() {
    let (_dir, store) = temp_store().await;
    let pending = store
        .create_account(NewAccount {
            username: sanitize_username("carol").unwrap(),
            role: Role::Sysop,
            state: AccountState::Pending,
        })
        .await
        .unwrap();
    store
        .attach_fingerprint(pending.id, &fingerprint(3))
        .await
        .unwrap();
    assert_eq!(store.role_of(&fingerprint(3)).await, None);
    assert_eq!(store.role_of(&fingerprint(4)).await, None);
}
