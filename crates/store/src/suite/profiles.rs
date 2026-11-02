//! Profile and member-list behaviour, against any backend (M2-F).
#![allow(clippy::unwrap_used, clippy::assert_is_empty)]

use russh_bbs_core::access::Slug;
use russh_bbs_core::profiles::{
    Cursor, Field, HandlePrefix, MemberQuery, Profile, ProfileStore, RawProfile,
};
use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::store::{
    Account, AccountId, AccountState, AccountStore, NewAccount, StoreError,
};

use super::Harness;

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

fn profile(name: &str, location: &str) -> Profile {
    Profile::from_raw(&RawProfile {
        display_name: name.to_owned(),
        location: location.to_owned(),
        ..RawProfile::default()
    })
}

async fn handles<S: ProfileStore>(store: &S, query: &MemberQuery, limit: usize) -> Vec<String> {
    store
        .list_members(query, limit)
        .await
        .unwrap()
        .iter()
        .map(|m| m.handle.as_str().to_owned())
        .collect()
}

pub(crate) async fn an_active_member_without_a_profile_has_an_empty_one<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let alice = account(store, "alice", AccountState::Active).await;
    let record = store.member(alice.id).await.unwrap().unwrap();
    assert_eq!(record.handle.as_str(), "alice");
    assert_eq!(record.profile, Profile::default());
}

pub(crate) async fn a_saved_profile_round_trips_and_a_later_save_replaces_it<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let alice = account(store, "alice", AccountState::Active).await;
    let bob = account(store, "bob", AccountState::Active).await;
    let first = Profile::from_raw(&RawProfile {
        display_name: "Alice A.".to_owned(),
        pronouns: "she/her".to_owned(),
        location: "Portland".to_owned(),
        about: "I run the board.".to_owned(),
        website: "example.org".to_owned(),
    });
    store.save_profile(alice.id, &first).await.unwrap();
    assert_eq!(
        store.member(alice.id).await.unwrap().unwrap().profile,
        first
    );
    store
        .save_profile(alice.id, &profile("Al", ""))
        .await
        .unwrap();
    let second = store.member(alice.id).await.unwrap().unwrap().profile;
    assert_eq!(second.get(Field::DisplayName), "Al");
    assert_eq!(
        second.get(Field::Location),
        "",
        "a save replaces every field"
    );
    // Profiles are per account.
    assert_eq!(
        store.member(bob.id).await.unwrap().unwrap().profile,
        Profile::default()
    );
}

pub(crate) async fn saving_a_profile_for_an_unknown_account_fails<H: Harness>() {
    let h = H::fresh().await;
    let result = h
        .store()
        .save_profile(AccountId::from_raw(9_999), &profile("x", ""))
        .await;
    assert!(
        matches!(result, Err(StoreError::AccountNotFound)),
        "{result:?}"
    );
}

pub(crate) async fn only_active_accounts_have_visible_profiles<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let active = account(store, "alice", AccountState::Active).await;
    let mut others = Vec::new();
    for (name, state) in [
        ("pending_pat", AccountState::Pending),
        ("approved_ann", AccountState::Approved),
        ("suspended_sam", AccountState::Suspended),
    ] {
        let other = account(store, name, state).await;
        // A profile row can exist for any account; it must not make the account visible.
        store
            .save_profile(other.id, &profile("Hidden", "Nowhere"))
            .await
            .unwrap();
        others.push(other.id);
    }
    assert_eq!(handles(store, &MemberQuery::default(), 50).await, ["alice"]);
    assert!(store.member(active.id).await.unwrap().is_some());
    for id in others {
        assert!(store.member(id).await.unwrap().is_none());
    }
    assert!(
        store
            .member(AccountId::from_raw(9_999))
            .await
            .unwrap()
            .is_none()
    );
    // A prefix search cannot reveal them either.
    let query = MemberQuery {
        prefix: HandlePrefix::parse("susp"),
        after: None,
    };
    assert!(handles(store, &query, 50).await.is_empty());
}

pub(crate) async fn the_list_is_sorted_by_handle_ignoring_case<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let names = ["Bob", "alice", "Zed", "carol", "al_ex", "9lives", "Alpha"];
    for name in names {
        account(store, name, AccountState::Active).await;
    }
    let mut expected: Vec<String> = names.iter().map(|n| (*n).to_owned()).collect();
    expected.sort_by_key(|n| n.to_ascii_lowercase());
    assert_eq!(handles(store, &MemberQuery::default(), 50).await, expected);
}

pub(crate) async fn a_prefix_matches_the_start_of_a_handle_only_and_has_no_wildcards<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    for name in ["Alice", "alpha", "balice", "a_c", "abc", "x-ray", "xylo"] {
        account(store, name, AccountState::Active).await;
    }
    let by = |typed: &str| MemberQuery {
        prefix: HandlePrefix::parse(typed),
        after: None,
    };
    assert_eq!(handles(store, &by("AL"), 50).await, ["Alice", "alpha"]);
    assert_eq!(
        handles(store, &by("a_"), 50).await,
        ["a_c"],
        "`_` is not a wildcard"
    );
    assert_eq!(handles(store, &by("x-"), 50).await, ["x-ray"]);
    assert!(
        handles(store, &by("lice"), 50).await.is_empty(),
        "start of the handle only"
    );
    assert!(handles(store, &by("zzz"), 50).await.is_empty());
    // `%` is not a handle character, so it is dropped and the prefix is empty: everyone.
    assert_eq!(handles(store, &by("%"), 50).await.len(), 7);
}

pub(crate) async fn paging_by_cursor_neither_skips_nor_repeats<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    for n in 0..23 {
        account(store, &format!("user{n:02}"), AccountState::Active).await;
    }
    let mut seen = Vec::new();
    let mut after: Option<Cursor> = None;
    loop {
        let page = store
            .list_members(
                &MemberQuery {
                    prefix: HandlePrefix::default(),
                    after: after.clone(),
                },
                5,
            )
            .await
            .unwrap();
        if page.is_empty() {
            break;
        }
        after = page.last().map(Cursor::after);
        seen.extend(page.iter().map(|m| m.handle.as_str().to_owned()));
    }
    let expected: Vec<String> = (0..23).map(|n| format!("user{n:02}")).collect();
    assert_eq!(seen, expected);
}

pub(crate) async fn the_list_carries_the_display_name<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let alice = account(store, "alice", AccountState::Active).await;
    account(store, "bob", AccountState::Active).await;
    store
        .save_profile(alice.id, &profile("Alice A.", ""))
        .await
        .unwrap();
    let rows = store
        .list_members(&MemberQuery::default(), 10)
        .await
        .unwrap();
    assert_eq!(rows[0].display_name, "Alice A.");
    assert_eq!(rows[1].display_name, "");
}

pub(crate) async fn profiles_survive_a_reopen<H: Harness>() {
    let mut h = H::fresh().await;
    let alice = account(h.store(), "alice", AccountState::Active).await;
    h.store()
        .save_profile(alice.id, &profile("Alice", "Here"))
        .await
        .unwrap();
    h.reopen().await;
    let record = h.store().member(alice.id).await.unwrap().unwrap();
    assert_eq!(record.profile.get(Field::Location), "Here");
}

pub(crate) async fn a_broken_store_fails_instead_of_answering<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let alice = account(store, "alice", AccountState::Active).await;
    h.close().await;
    assert!(
        store
            .list_members(&MemberQuery::default(), 5)
            .await
            .is_err()
    );
    assert!(store.member(alice.id).await.is_err());
    assert!(
        store
            .save_profile(alice.id, &profile("x", ""))
            .await
            .is_err()
    );
}
