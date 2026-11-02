//! Terminal settings and login times, against any backend.
#![allow(clippy::unwrap_used)]

use russh_bbs_core::access::{AccessStore, Permission, Slug};
use russh_bbs_core::icons::Tier;
use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::store::{
    Account, AccountId, AccountState, AccountStore, NewAccount, StoreError,
};
use russh_bbs_core::terminal_settings::{MemberSessions, TerminalSettings, TerminalSettingsStore};

use super::{Harness, fingerprint};

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

pub(crate) async fn defaults_when_missing<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let alice = account(store, "alice", AccountState::Active).await;
    let settings = store.terminal_settings(alice.id).await.unwrap();
    assert_eq!(settings, TerminalSettings::default());
    assert!(settings.needs_glyph_test());
    assert!(settings.pause());
}

pub(crate) async fn set_then_get_round_trips<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
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

pub(crate) async fn every_tier_and_unset_round_trip<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
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

pub(crate) async fn a_later_set_replaces_and_clears_overrides<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
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

pub(crate) async fn accounts_are_isolated<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
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

pub(crate) async fn saving_for_an_unknown_account_fails<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
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

pub(crate) async fn login_times_report_the_previous_login<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
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

pub(crate) async fn sessions_start_from_stored_state<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let alice = account(store, "alice", AccountState::Active).await;
    let key = fingerprint(1);
    store.attach_fingerprint(alice.id, &key).await.unwrap();

    let first = store.start_for_key(&key).await.unwrap().unwrap();
    assert_eq!(first.account, alice.id);
    assert_eq!(first.handle.as_str(), "alice");
    assert_eq!(first.settings, TerminalSettings::default());
    assert_eq!(first.previous_login, None);

    let chosen = first.settings.with_tier(Tier::Unicode);
    let granted = first
        .access
        .require(Permission::AccountEditSettings, None)
        .unwrap();
    store.save_settings(&granted, &chosen).await.unwrap();
    // Proof of some other permission does not save settings, and an account that cannot hold
    // the permission cannot get the proof at all.
    let wrong = first.access.require(Permission::MailRead, None).unwrap();
    assert!(matches!(
        store.save_settings(&wrong, &chosen.with_color(false)).await,
        Err(russh_bbs_core::terminal_settings::SaveError::NotPermitted)
    ));
    let waiting = store.snapshot(None).await.unwrap();
    assert!(
        waiting
            .require(Permission::AccountEditSettings, None)
            .is_err()
    );
    let second = store.start_for_key(&key).await.unwrap().unwrap();
    assert_eq!(second.settings, chosen, "the stored tier is used");
    assert!(second.previous_login.is_some());
}

pub(crate) async fn only_active_accounts_start_a_session<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
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

pub(crate) async fn settings_survive_reopening_the_database<H: Harness>() {
    let mut h = H::fresh().await;
    let alice = account(h.store(), "alice", AccountState::Active).await;
    let wanted = TerminalSettings::default().with_tier(Tier::NerdFont);
    h.store()
        .set_terminal_settings(alice.id, &wanted)
        .await
        .unwrap();
    h.reopen().await;
    assert_eq!(h.store().terminal_settings(alice.id).await.unwrap(), wanted);
}

pub(crate) async fn concurrent_logins_each_see_a_consistent_previous_time<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store().clone();
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
