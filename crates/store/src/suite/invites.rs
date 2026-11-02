//! Invites, the invite tree and account state changes against any backend (M2-D): hashed
//! storage, single use, expiry by the store's clock, the quota, atomic redemption under
//! concurrency, and that every change lands with its audit row in its own transaction.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::sync::Arc;

use russh_bbs_core::access::{Fingerprint, Slug};
use russh_bbs_core::audit::{
    Action, Actor, AuditEntry, AuditQuery, AuditStore, FixedClock, Target,
};
use russh_bbs_core::invites::{
    CODE_BYTES, CodeHash, CreateError, INVITE_TTL_SECS, InviteCode, InviteRecord, InviteStatus,
    InviteStore, NewInvite, QUOTA_WINDOW_SECS, Quota, RedeemError, Redeemed, RejectReason,
};
use russh_bbs_core::sanitize::{Username, sanitize_username};
use russh_bbs_core::store::{
    Account, AccountId, AccountState, AccountStateStore, AccountStore, NewAccount,
    StateChangeError, StoreError,
};

use super::{Harness, TestStore, fingerprint, top_role_of};

fn clock_at(h: &mut impl Harness, at: i64) -> Arc<FixedClock> {
    let clock = Arc::new(FixedClock::new(at));
    h.set_clock(clock.clone());
    clock
}

fn name(text: &str) -> Username {
    sanitize_username(text).unwrap()
}

async fn account_in<S: AccountStore>(store: &S, text: &str, state: AccountState) -> Account {
    store
        .create_account(NewAccount {
            username: name(text),
            role: Slug::MEMBER,
            state,
        })
        .await
        .unwrap()
}

async fn member<S: AccountStore>(store: &S, text: &str) -> Account {
    account_in(store, text, AccountState::Active).await
}

/// Make a code from `seed` for `inviter`.
async fn make<S: InviteStore>(
    store: &S,
    inviter: &Account,
    seed: u8,
    quota: Quota,
) -> (InviteRecord, InviteCode) {
    let code = InviteCode::from_bytes([seed; CODE_BYTES]);
    let record = store
        .create_invite(&NewInvite {
            inviter: inviter.id,
            code_hash: code.hash(),
            ttl_secs: INVITE_TTL_SECS,
            quota,
        })
        .await
        .unwrap();
    (record, code)
}

async fn redeem<S: InviteStore>(
    store: &S,
    code: &InviteCode,
    handle: &str,
    key: &Fingerprint,
) -> Result<Redeemed, RedeemError> {
    store.redeem_invite(&code.hash(), name(handle), key).await
}

fn is_code_refusal<T: std::fmt::Debug>(result: &Result<T, RedeemError>) -> bool {
    matches!(result, Err(RedeemError::Rejected(RejectReason::Code)))
}

async fn all<S: AuditStore>(store: &S) -> Vec<AuditEntry> {
    store.page(&AuditQuery::default()).await.unwrap()
}

fn count_of(rows: &[AuditEntry], action: Action) -> usize {
    rows.iter().filter(|r| r.event.action == action).count()
}

async fn count<H: Harness>(h: &H, from_where: &str) -> String {
    h.texts(&format!("SELECT CAST(count(*) AS TEXT) FROM {from_where}"))
        .await
        .remove(0)
}

pub(crate) async fn a_created_invite_is_stored_hashed_bound_to_the_inviter_and_audited<
    H: Harness,
>() {
    let mut h = H::fresh().await;
    clock_at(&mut h, 1_000);
    let store = h.store();
    let alice = member(store, "alice").await;
    let (record, code) = make(store, &alice, 1, Quota::Limited(3)).await;
    assert_eq!(record.expires_at.as_raw(), 1_000 + INVITE_TTL_SECS);
    assert_eq!(record.used_in_window, 1);

    // The database holds the hash and nothing from which the code could be read back.
    assert_eq!(
        h.texts("SELECT code_hash FROM invite").await,
        [code.hash().to_hex()]
    );
    let plain = code.reveal().replace('-', "");
    let everything = h
        .texts("SELECT CAST(id AS TEXT) || ' ' || code_hash || ' ' || CAST(inviter_id AS TEXT) FROM invite")
        .await
        .join("\n");
    assert!(!everything.contains(&plain));
    let inviter_of = h.texts("SELECT CAST(inviter_id AS TEXT) FROM invite").await;
    assert_eq!(inviter_of, [alice.id.to_string()]);

    // One audit row, naming the invite by id; the code is nowhere in the log.
    let rows = all(h.store()).await;
    let created: Vec<_> = rows
        .iter()
        .filter(|r| r.event.action == Action::InviteCreated)
        .collect();
    assert_eq!(created.len(), 1);
    assert_eq!(created[0].event.actor, Actor::Account(alice.id));
    assert_eq!(created[0].event.target, Target::Invite(record.id));
    assert_eq!(created[0].at.as_raw(), 1_000);
    let log = h
        .texts("SELECT action || ' ' || target_kind || ' ' || target_ref || ' ' || detail FROM audit_log")
        .await
        .join("\n");
    assert!(!log.contains(&plain));
    assert!(!log.contains(&code.hash().to_hex()));
}

pub(crate) async fn redeeming_makes_an_active_member_with_the_inviter_recorded<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let alice = member(store, "alice").await;
    let (record, code) = make(store, &alice, 2, Quota::Limited(3)).await;
    let key = fingerprint(10);
    let got = redeem(store, &code, "newbie", &key).await.unwrap();
    assert_eq!(got.account.state, AccountState::Active);
    assert_eq!(got.account.username.as_str(), "newbie");
    assert_eq!(got.inviter, alice.id);
    assert_eq!(got.invite, record.id);

    assert!(russh_bbs_core::access::MemberDirectory::is_member(store, &key).await);
    let found = store.account_by_fingerprint(&key).await.unwrap().unwrap();
    assert_eq!(found.id, got.account.id);
    assert_eq!(top_role_of(store, &key).await, Some(Slug::MEMBER));

    let tree = store.invite_tree(100).await.unwrap();
    let node = tree.iter().find(|n| n.account == got.account.id).unwrap();
    assert_eq!(node.invited_by, Some(alice.id));
    let alice_node = tree.iter().find(|n| n.account == alice.id).unwrap();
    assert_eq!(alice_node.invited_by, None);

    // Both audit rows: the account made by the redemption, and the redemption itself.
    let rows = all(store).await;
    let made = rows
        .iter()
        .find(|r| {
            r.event.action == Action::AccountCreated
                && r.event.target == Target::Account(got.account.id)
        })
        .unwrap();
    assert_eq!(made.event.actor, Actor::Account(got.account.id));
    assert!(made.event.detail.as_str().contains("via invite"));
    let redeemed = rows
        .iter()
        .find(|r| r.event.action == Action::InviteRedeemed)
        .unwrap();
    assert_eq!(redeemed.event.target, Target::Invite(record.id));
    assert_eq!(redeemed.event.actor, Actor::Account(got.account.id));
}

pub(crate) async fn a_code_works_once<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let alice = member(store, "alice").await;
    let (_, code) = make(store, &alice, 3, Quota::Limited(3)).await;
    redeem(store, &code, "first", &fingerprint(10))
        .await
        .unwrap();
    let again = redeem(store, &code, "second", &fingerprint(11)).await;
    assert!(is_code_refusal(&again), "{again:?}");
    assert!(
        store
            .account_by_fingerprint(&fingerprint(11))
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(count(&h, "accounts").await, "2");
}

pub(crate) async fn every_kind_of_bad_code_is_the_same_refusal<H: Harness>() {
    let mut h = H::fresh().await;
    let clock = clock_at(&mut h, 1_000);
    let store = h.store();
    let alice = member(store, "alice").await;
    let (_, used) = make(store, &alice, 4, Quota::Limited(9)).await;
    redeem(store, &used, "taken_up", &fingerprint(10))
        .await
        .unwrap();
    let (_, expired) = make(store, &alice, 5, Quota::Limited(9)).await;
    let (_, good) = make(store, &alice, 6, Quota::Limited(9)).await;

    clock.set(1_000 + INVITE_TTL_SECS + 1);
    let attempts = [
        (
            "unknown",
            store
                .redeem_invite(&CodeHash::of("nonsense"), name("nobody"), &fingerprint(20))
                .await,
        ),
        (
            "empty",
            store
                .redeem_invite(&CodeHash::of(""), name("nobody"), &fingerprint(21))
                .await,
        ),
        (
            "used",
            redeem(store, &used, "again", &fingerprint(22)).await,
        ),
        (
            "expired",
            redeem(store, &expired, "late", &fingerprint(23)).await,
        ),
        (
            "expired too",
            redeem(store, &good, "late2", &fingerprint(24)).await,
        ),
        (
            "one character off",
            store
                .redeem_invite(
                    &CodeHash::of(&good.reveal().replace('-', "")[1..]),
                    name("typo"),
                    &fingerprint(25),
                )
                .await,
        ),
    ];
    for (what, result) in &attempts {
        assert!(is_code_refusal(result), "{what}: {result:?}");
    }
    // Nothing was created by any of them.
    assert_eq!(count(&h, "accounts").await, "2");
    assert_eq!(count(&h, "account_keys").await, "1");
}

pub(crate) async fn expiry_is_exact_and_uses_the_store_clock<H: Harness>() {
    let mut h = H::fresh().await;
    let clock = clock_at(&mut h, 1_000);
    let store = h.store();
    let alice = member(store, "alice").await;
    let (a, code_a) = make(store, &alice, 7, Quota::Limited(9)).await;
    let (_, code_b) = make(store, &alice, 8, Quota::Limited(9)).await;
    assert_eq!(a.expires_at.as_raw(), 1_000 + INVITE_TTL_SECS);

    // One second before expiry it works; at the expiry second it does not.
    clock.set(a.expires_at.as_raw() - 1);
    redeem(store, &code_a, "justin", &fingerprint(10))
        .await
        .unwrap();
    clock.set(a.expires_at.as_raw());
    let late = redeem(store, &code_b, "tooslow", &fingerprint(11)).await;
    assert!(is_code_refusal(&late), "{late:?}");
}

pub(crate) async fn a_refused_redemption_consumes_nothing<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let alice = member(store, "alice").await;
    let bob = member(store, "bob").await;
    store
        .attach_fingerprint(bob.id, &fingerprint(5))
        .await
        .unwrap();
    let (_, code) = make(store, &alice, 9, Quota::Limited(9)).await;

    // A taken handle, a lookalike of a taken handle, and a key that already has an account.
    let taken = redeem(store, &code, "alice", &fingerprint(10)).await;
    assert!(
        matches!(taken, Err(RedeemError::Rejected(RejectReason::Handle))),
        "{taken:?}"
    );
    let lookalike = redeem(store, &code, "a1ice", &fingerprint(11)).await;
    assert!(
        matches!(lookalike, Err(RedeemError::Rejected(RejectReason::Handle))),
        "{lookalike:?}"
    );
    let bound = redeem(store, &code, "fresh_name", &fingerprint(5)).await;
    assert!(
        matches!(bound, Err(RedeemError::Rejected(RejectReason::Key))),
        "{bound:?}"
    );

    // Nothing was written: no account, no key, no redemption row, and the code is still good.
    assert_eq!(count(&h, "accounts").await, "2");
    assert_eq!(count(&h, "invite WHERE redeemed_at IS NOT NULL").await, "0");
    let rows = all(store).await;
    assert_eq!(count_of(&rows, Action::InviteRedeemed), 0);
    redeem(store, &code, "fresh_name", &fingerprint(12))
        .await
        .unwrap();
    assert_eq!(count_of(&all(store).await, Action::InviteRedeemed), 1);
}

pub(crate) async fn a_suspended_inviter_voids_their_open_codes<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let alice = member(store, "alice").await;
    let (_, code) = make(store, &alice, 10, Quota::Limited(9)).await;
    store
        .change_state(alice.id, AccountState::Suspended, Actor::System)
        .await
        .unwrap();
    let refused = redeem(store, &code, "newbie", &fingerprint(10)).await;
    assert!(is_code_refusal(&refused), "{refused:?}");
    // Reinstated, the same code works again: it was never consumed.
    store
        .change_state(alice.id, AccountState::Active, Actor::System)
        .await
        .unwrap();
    redeem(store, &code, "newbie", &fingerprint(10))
        .await
        .unwrap();
}

pub(crate) async fn concurrent_redemptions_of_one_code_make_exactly_one_account<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let alice = member(store, "alice").await;
    for round in 0..3_u8 {
        let (_, code) = make(store, &alice, 20 + round, Quota::Unlimited).await;
        let hash = code.hash();
        let mut tasks = Vec::new();
        for i in 0..8_u32 {
            let store = store.clone();
            tasks.push(tokio::spawn(async move {
                let n = u32::from(round) * 100 + i;
                store
                    .redeem_invite(&hash, name(&format!("racer{n}")), &fingerprint(100 + n))
                    .await
            }));
        }
        let mut accepted = 0;
        for task in tasks {
            match task.await.unwrap() {
                Ok(_) => accepted += 1,
                Err(error) => assert!(
                    matches!(error, RedeemError::Rejected(RejectReason::Code)),
                    "{error:?}"
                ),
            }
        }
        assert_eq!(accepted, 1, "round {round}");
    }
    // alice plus one per round, one redemption row and one pair of audit rows per round.
    assert_eq!(count(&h, "accounts").await, "4");
    assert_eq!(count(&h, "account_keys").await, "3");
    assert_eq!(count_of(&all(store).await, Action::InviteRedeemed), 3);
}

pub(crate) async fn the_quota_counts_a_rolling_window_and_a_missing_inviter_is_an_error<
    H: Harness,
>() {
    let mut h = H::fresh().await;
    let clock = clock_at(&mut h, 1_000);
    let store = h.store();
    let alice = member(store, "alice").await;
    let bob = member(store, "bob").await;
    let (first, _) = make(store, &alice, 30, Quota::Limited(2)).await;
    let (second, _) = make(store, &alice, 31, Quota::Limited(2)).await;
    assert_eq!((first.used_in_window, second.used_in_window), (1, 2));

    let over = store
        .create_invite(&NewInvite {
            inviter: alice.id,
            code_hash: InviteCode::from_bytes([32; CODE_BYTES]).hash(),
            ttl_secs: INVITE_TTL_SECS,
            quota: Quota::Limited(2),
        })
        .await;
    assert!(
        matches!(over, Err(CreateError::QuotaReached { limit: 2 })),
        "{over:?}"
    );
    // The refusal wrote no invite and no audit row.
    assert_eq!(count(&h, "invite").await, "2");
    assert_eq!(count_of(&all(store).await, Action::InviteCreated), 2);

    // Another inviter has their own count; no quota never refuses.
    make(store, &bob, 33, Quota::Limited(2)).await;
    for seed in 40..45 {
        make(store, &alice, seed, Quota::Unlimited).await;
    }

    // The window is rolling: a code made exactly one window ago no longer counts.
    clock.set(1_000 + QUOTA_WINDOW_SECS);
    let (renewed, _) = make(store, &bob, 50, Quota::Limited(2)).await;
    assert_eq!(
        renewed.used_in_window, 1,
        "bob's first code has left the window"
    );
    let carol = member(store, "carol").await;
    let (fresh, _) = make(store, &carol, 51, Quota::Limited(1)).await;
    assert_eq!(fresh.used_in_window, 1);

    let missing = store
        .create_invite(&NewInvite {
            inviter: AccountId::from_raw(9_999),
            code_hash: InviteCode::from_bytes([60; CODE_BYTES]).hash(),
            ttl_secs: INVITE_TTL_SECS,
            quota: Quota::Unlimited,
        })
        .await;
    assert!(
        matches!(
            missing,
            Err(CreateError::Store(StoreError::AccountNotFound))
        ),
        "{missing:?}"
    );
}

pub(crate) async fn concurrent_creations_cannot_exceed_the_quota<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let alice = member(store, "alice").await;
    let mut tasks = Vec::new();
    for i in 0..8_u8 {
        let store = store.clone();
        let inviter = alice.id;
        tasks.push(tokio::spawn(async move {
            store
                .create_invite(&NewInvite {
                    inviter,
                    code_hash: InviteCode::from_bytes([70 + i; CODE_BYTES]).hash(),
                    ttl_secs: INVITE_TTL_SECS,
                    quota: Quota::Limited(3),
                })
                .await
        }));
    }
    let (mut made, mut refused) = (0, 0);
    for task in tasks {
        match task.await.unwrap() {
            Ok(_) => made += 1,
            Err(CreateError::QuotaReached { limit: 3 }) => refused += 1,
            Err(other) => panic!("{other:?}"),
        }
    }
    assert_eq!((made, refused), (3, 5));
    assert_eq!(count(&h, "invite").await, "3");
    assert_eq!(count_of(&all(store).await, Action::InviteCreated), 3);
}

pub(crate) async fn the_tree_lists_members_with_their_inviters_and_never_applicants<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let founder = member(store, "founder").await;
    let (_, c1) = make(store, &founder, 80, Quota::Unlimited).await;
    let alice = redeem(store, &c1, "alice", &fingerprint(10)).await.unwrap();
    let (_, c2) = make(store, &alice.account, 81, Quota::Unlimited).await;
    let bob = redeem(store, &c2, "bob", &fingerprint(11)).await.unwrap();
    account_in(store, "applicant", AccountState::Pending).await;
    account_in(store, "approved", AccountState::Approved).await;
    store
        .change_state(bob.account.id, AccountState::Suspended, Actor::System)
        .await
        .unwrap();

    let tree = store.invite_tree(100).await.unwrap();
    let shape: Vec<(String, Option<String>)> = tree
        .iter()
        .map(|n| {
            let by = n
                .invited_by
                .and_then(|id| tree.iter().find(|m| m.account == id))
                .map(|m| m.handle.as_str().to_owned());
            (n.handle.as_str().to_owned(), by)
        })
        .collect();
    assert_eq!(
        shape,
        [
            ("founder".to_owned(), None),
            ("alice".to_owned(), Some("founder".to_owned())),
            ("bob".to_owned(), Some("alice".to_owned())),
        ],
        "active and suspended members only, oldest first, never a pending or approved account"
    );
    // The limit bounds the read.
    assert_eq!(store.invite_tree(2).await.unwrap().len(), 2);
}

pub(crate) async fn my_invites_are_newest_first_with_their_status<H: Harness>() {
    let mut h = H::fresh().await;
    let clock = clock_at(&mut h, 1_000);
    let store = h.store();
    let alice = member(store, "alice").await;
    let bob = member(store, "bob").await;
    let (a, _code_a) = make(store, &alice, 90, Quota::Unlimited).await;
    clock.set(1_100);
    let (b, code_b) = make(store, &alice, 91, Quota::Unlimited).await;
    clock.set(1_200);
    let (c, _code_c) = make(store, &alice, 92, Quota::Unlimited).await;
    make(store, &bob, 93, Quota::Unlimited).await;
    clock.set(1_300);
    redeem(store, &code_b, "newbie", &fingerprint(10))
        .await
        .unwrap();
    clock.set(1_000 + INVITE_TTL_SECS + 50);

    let mine = store.invites_by(alice.id, 10).await.unwrap();
    let ids: Vec<_> = mine.iter().map(|i| i.id).collect();
    assert_eq!(ids, [c.id, b.id, a.id], "newest first, only alice's");
    assert_eq!(mine[0].status, InviteStatus::Open);
    assert_eq!(mine[1].status, InviteStatus::Redeemed(Some(name("newbie"))));
    assert_eq!(mine[2].status, InviteStatus::Expired);
    assert_eq!(mine[0].created_at.as_raw(), 1_200);
    assert_eq!(mine[0].expires_at.as_raw(), 1_200 + INVITE_TTL_SECS);
    assert_eq!(store.invites_by(alice.id, 2).await.unwrap().len(), 2);
    assert_eq!(store.invites_by(bob.id, 10).await.unwrap().len(), 1);
}

pub(crate) async fn deleting_accounts_keeps_invites_and_the_tree_intact<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let alice = member(store, "alice").await;
    let (_, c1) = make(store, &alice, 100, Quota::Unlimited).await;
    let newbie = redeem(store, &c1, "newbie", &fingerprint(10))
        .await
        .unwrap();
    let (_, c2) = make(store, &newbie.account, 101, Quota::Unlimited).await;
    let carol = redeem(store, &c2, "carol", &fingerprint(11)).await.unwrap();

    // Nothing but the deleted account's own rows goes with it: its key goes (cascade), the code
    // it redeemed stays and loses the link (set null), the codes it made go with their maker, and
    // an account it invited stays and loses the link.
    let deleted = h
        .raw("DELETE FROM accounts WHERE username = 'newbie'")
        .await
        .unwrap();
    assert_eq!(deleted, 1);
    assert!(
        store
            .account_by_fingerprint(&fingerprint(10))
            .await
            .unwrap()
            .is_none()
    );
    let mine = store.invites_by(alice.id, 10).await.unwrap();
    assert_eq!(mine.len(), 1);
    assert_eq!(mine[0].status, InviteStatus::Redeemed(None));
    let tree = store.invite_tree(10).await.unwrap();
    let carol_node = tree.iter().find(|n| n.account == carol.account.id).unwrap();
    assert_eq!(carol_node.invited_by, None);
    assert_eq!(count(&h, "invite").await, "1");
    assert!(
        store
            .account_by_fingerprint(&fingerprint(11))
            .await
            .unwrap()
            .is_some()
    );
}

pub(crate) async fn invite_and_state_writes_never_land_without_their_audit_row<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let alice = member(store, "alice").await;
    let (_, code) = make(store, &alice, 110, Quota::Unlimited).await;
    let pending = account_in(store, "pending_one", AccountState::Pending).await;
    h.break_audit_inserts().await;

    // Creating, redeeming and changing a state each fail whole when their audit row cannot be
    // written.
    let created = store
        .create_invite(&NewInvite {
            inviter: alice.id,
            code_hash: InviteCode::from_bytes([111; CODE_BYTES]).hash(),
            ttl_secs: INVITE_TTL_SECS,
            quota: Quota::Unlimited,
        })
        .await;
    assert!(matches!(created, Err(CreateError::Store(_))), "{created:?}");
    assert_eq!(count(&h, "invite").await, "1");

    let redeemed = redeem(store, &code, "newbie", &fingerprint(10)).await;
    assert!(
        matches!(redeemed, Err(RedeemError::Store(_))),
        "{redeemed:?}"
    );
    assert_eq!(count(&h, "accounts").await, "2");
    assert_eq!(count(&h, "account_keys").await, "0");
    assert_eq!(count(&h, "invite WHERE redeemed_at IS NULL").await, "1");

    let changed = store
        .change_state(pending.id, AccountState::Active, Actor::System)
        .await;
    assert!(
        matches!(changed, Err(StateChangeError::Store(_))),
        "{changed:?}"
    );
    let still = h
        .texts(&format!(
            "SELECT state FROM accounts WHERE id = {}",
            pending.id
        ))
        .await;
    assert_eq!(still, ["pending"]);
}

pub(crate) async fn invites_survive_a_reopen<H: Harness>() {
    let mut h = H::fresh().await;
    let alice = member(h.store(), "alice").await;
    let (record, code) = make(h.store(), &alice, 120, Quota::Limited(3)).await;
    h.reopen().await;
    let store = h.store();
    let listed = store.invites_by(alice.id, 10).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, record.id);
    redeem(store, &code, "newbie", &fingerprint(10))
        .await
        .unwrap();
}

pub(crate) async fn allowed_state_changes_are_made_and_audited<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let admin = member(store, "keeper").await;
    let person = account_in(store, "person", AccountState::Pending).await;
    store
        .attach_fingerprint(person.id, &fingerprint(5))
        .await
        .unwrap();
    let key = fingerprint(5);
    let path = [
        AccountState::Approved,
        AccountState::Active,
        AccountState::Suspended,
        AccountState::Active,
    ];
    let mut from = AccountState::Pending;
    for to in path {
        let change = store
            .change_state(person.id, to, Actor::Account(admin.id))
            .await
            .unwrap();
        assert_eq!((change.from, change.to), (from, to));
        // Only an active account is a member and holds more than the waiting room.
        let role = top_role_of(store, &key).await;
        if to == AccountState::Active {
            assert_eq!(role, Some(Slug::MEMBER));
            assert!(russh_bbs_core::access::MemberDirectory::is_member(store, &key).await);
        } else {
            assert_eq!(role, Some(Slug::WAITING), "{to:?}");
            assert!(!russh_bbs_core::access::MemberDirectory::is_member(store, &key).await);
        }
        from = to;
    }
    let rows = all(store).await;
    let changes: Vec<&AuditEntry> = rows
        .iter()
        .filter(|r| r.event.action == Action::AccountStateChanged)
        .collect();
    assert_eq!(changes.len(), 4);
    // Newest first.
    let details: Vec<&str> = changes.iter().map(|r| r.event.detail.as_str()).collect();
    assert_eq!(
        details,
        [
            "suspended to active",
            "active to suspended",
            "approved to active",
            "pending to approved"
        ]
    );
    assert!(
        changes
            .iter()
            .all(|r| r.event.actor == Actor::Account(admin.id))
    );
    assert!(
        changes
            .iter()
            .all(|r| r.event.target == Target::Account(person.id))
    );
}

pub(crate) async fn refused_state_changes_write_nothing<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let active = member(store, "active_one").await;
    let before = all(store).await.len();
    for to in [
        AccountState::Pending,
        AccountState::Approved,
        AccountState::Active,
    ] {
        let refused = store.change_state(active.id, to, Actor::System).await;
        assert!(
            matches!(refused, Err(StateChangeError::NotAllowed { from: AccountState::Active, to: t }) if t == to),
            "{to:?}: {refused:?}"
        );
    }
    let missing = store
        .change_state(
            AccountId::from_raw(9_999),
            AccountState::Active,
            Actor::System,
        )
        .await;
    assert!(
        matches!(missing, Err(StateChangeError::NotFound)),
        "{missing:?}"
    );
    assert_eq!(
        all(store).await.len(),
        before,
        "a refusal writes no audit row"
    );
    assert_eq!(
        h.texts(&format!(
            "SELECT state FROM accounts WHERE id = {}",
            active.id
        ))
        .await,
        ["active"]
    );
}

pub(crate) async fn racing_state_changes_apply_once<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let person = member(store, "person").await;
    let mut tasks = Vec::new();
    for _ in 0..6 {
        let store = store.clone();
        tasks.push(tokio::spawn(async move {
            store
                .change_state(person.id, AccountState::Suspended, Actor::System)
                .await
        }));
    }
    let (mut done, mut refused) = (0, 0);
    for task in tasks {
        match task.await.unwrap() {
            Ok(_) => done += 1,
            Err(StateChangeError::NotAllowed {
                from: AccountState::Suspended,
                to: AccountState::Suspended,
            }) => refused += 1,
            Err(other) => panic!("{other:?}"),
        }
    }
    assert_eq!((done, refused), (1, 5));
    assert_eq!(count_of(&all(store).await, Action::AccountStateChanged), 1);
}

#[allow(dead_code)]
fn _store_bound<S: TestStore>() {}
