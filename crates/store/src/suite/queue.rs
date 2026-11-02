//! The sysop queue against any backend (M2-E, D71, D72): what is stored (and what is not), the
//! one-request-per-key rule and the cap under concurrency, atomic decisions, vouching, the
//! denial cooldown and expiry with an injected clock, the purge, and that every decision lands
//! with its audit row in its own transaction and without any request content.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::sync::Arc;

use russh_bbs_core::access::Slug;
use russh_bbs_core::audit::{
    Action, Actor, AuditEntry, AuditQuery, AuditStore, FixedClock, Target,
};
use russh_bbs_core::queue::{
    ApproveError, DENIAL_COOLDOWN_SECS, DecideError, Message, NewRequest, PENDING_TTL_SECS, Queue,
    QueueStore, Refusal, RequestId, RequestKind, SubmitTerms, Submitted, VouchError, cooldown_key,
};
use russh_bbs_core::sanitize::{Username, sanitize_username};
use russh_bbs_core::store::{Account, AccountState, AccountStore, NewAccount, StoreError};

use super::{Harness, fingerprint, top_role_of};

const HANDLE: &str = "hopeful-handle";
const MESSAGE: &str = "a friend of somebody here";

fn clock_at(h: &mut impl Harness, at: i64) -> Arc<FixedClock> {
    let clock = Arc::new(FixedClock::new(at));
    h.set_clock(clock.clone());
    clock
}

fn name(text: &str) -> Username {
    sanitize_username(text).unwrap()
}

fn terms(cap: u32) -> SubmitTerms {
    SubmitTerms {
        max_pending: cap,
        ttl_secs: PENDING_TTL_SECS,
    }
}

fn request(key: u32, handle: &str) -> NewRequest {
    NewRequest {
        handle: name(handle),
        message: Message::new(MESSAGE),
        fingerprint: fingerprint(key),
        kind: RequestKind::Invite,
    }
}

async fn member<S: AccountStore>(store: &S, text: &str) -> Account {
    account_in(store, text, AccountState::Active).await
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

/// Submit and require that it was stored.
async fn stored<S: QueueStore>(store: &S, key: u32, handle: &str) -> RequestId {
    match store
        .submit_request(&request(key, handle), terms(200))
        .await
        .unwrap()
    {
        Submitted::Stored(id) => id,
        other @ Submitted::Refused(_) => panic!("expected the request to be stored, got {other:?}"),
    }
}

async fn queue<S: QueueStore>(store: &S) -> Queue {
    store.pending_requests(100).await.unwrap()
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

pub(crate) async fn a_request_stores_only_the_documented_fields_and_writes_no_audit_row<
    H: Harness,
>() {
    let mut h = H::fresh().await;
    clock_at(&mut h, 5_000);
    let store = h.store();
    let id = stored(store, 1, HANDLE).await;

    let row = h
        .texts(
            "SELECT fingerprint || '|' || handle || '|' || message || '|' \
             || CAST(created_at AS TEXT) || '|' || CAST(expires_at AS TEXT) FROM invite_request",
        )
        .await;
    assert_eq!(
        row,
        [format!(
            "{}|{HANDLE}|{MESSAGE}|5000|{}",
            fingerprint(1).as_str(),
            5_000 + PENDING_TTL_SECS
        )]
    );
    let listed = queue(store).await;
    assert_eq!(listed.now.as_raw(), 5_000);
    assert_eq!(listed.requests.len(), 1);
    assert_eq!(listed.requests[0].id, id);
    assert_eq!(listed.requests[0].handle.as_str(), HANDLE);
    assert_eq!(listed.requests[0].message.as_str(), MESSAGE);
    assert_eq!(listed.requests[0].vouchers.len(), 0);
    // A submission is deliberately not audited (an unauthenticated flood must not be able to
    // grow an append-only log).
    assert_eq!(all(store).await.len(), 0);
    assert_eq!(count(&h, "request_denial").await, "0");
}

pub(crate) async fn one_request_per_key_and_a_full_queue_refuses_with_the_slot_freed_by_a_decision<
    H: Harness,
>() {
    let h = H::fresh().await;
    let store = h.store();
    let first = stored(store, 1, "first-one").await;

    // The same key again, with different text: refused, and the first request is untouched.
    let again = store
        .submit_request(&request(1, "second-try"), terms(2))
        .await
        .unwrap();
    assert_eq!(again, Submitted::Refused(Refusal::AlreadyPending));
    let listed = queue(store).await.requests;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].handle.as_str(), "first-one");

    // A second key fits (cap 2); a third does not.
    let second = store
        .submit_request(&request(2, "second-one"), terms(2))
        .await
        .unwrap();
    assert!(matches!(second, Submitted::Stored(_)));
    let third = store
        .submit_request(&request(3, "third-one"), terms(2))
        .await
        .unwrap();
    assert_eq!(third, Submitted::Refused(Refusal::QueueFull));
    assert_eq!(count(&h, "invite_request").await, "2");

    // A decision frees a slot.
    let sysop = member(store, "sysop-person").await;
    store
        .deny_request(first, sysop.id, DENIAL_COOLDOWN_SECS)
        .await
        .unwrap();
    let third = store
        .submit_request(&request(3, "third-one"), terms(2))
        .await
        .unwrap();
    assert!(matches!(third, Submitted::Stored(_)));
}

pub(crate) async fn a_key_that_already_has_an_account_is_refused<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    for (n, state) in [
        (1, AccountState::Active),
        (2, AccountState::Pending),
        (3, AccountState::Suspended),
    ] {
        let owner = account_in(store, &format!("owner{n}"), state).await;
        store
            .attach_fingerprint(owner.id, &fingerprint(n))
            .await
            .unwrap();
        let answer = store
            .submit_request(&request(n, "hopeful-handle"), terms(200))
            .await
            .unwrap();
        assert_eq!(
            answer,
            Submitted::Refused(Refusal::KeyHasAccount),
            "{state:?}"
        );
    }
    assert_eq!(count(&h, "invite_request").await, "0");
}

pub(crate) async fn concurrent_submissions_never_pass_the_cap_or_the_one_per_key_rule<
    H: Harness,
>() {
    let h = H::fresh().await;
    let store = h.store();
    // Many keys, cap 3: exactly three are stored.
    let mut tasks = Vec::new();
    for n in 0..12_u32 {
        let store = store.clone();
        tasks.push(tokio::spawn(async move {
            store
                .submit_request(&request(100 + n, &format!("racer{n}")), terms(3))
                .await
                .unwrap()
        }));
    }
    let mut kept = 0;
    for task in tasks {
        match task.await.unwrap() {
            Submitted::Stored(_) => kept += 1,
            Submitted::Refused(refusal) => assert_eq!(refusal, Refusal::QueueFull),
        }
    }
    assert_eq!(kept, 3);
    assert_eq!(count(&h, "invite_request").await, "3");

    // One key, many tries, a roomy queue: exactly one is stored.
    let mut tasks = Vec::new();
    for n in 0..12_u32 {
        let store = store.clone();
        tasks.push(tokio::spawn(async move {
            store
                .submit_request(&request(500, &format!("same{n}")), terms(200))
                .await
                .unwrap()
        }));
    }
    let mut kept = 0;
    for task in tasks {
        match task.await.unwrap() {
            Submitted::Stored(_) => kept += 1,
            Submitted::Refused(refusal) => assert_eq!(refusal, Refusal::AlreadyPending),
        }
    }
    assert_eq!(kept, 1);
    assert_eq!(
        count(
            &h,
            &format!(
                "invite_request WHERE fingerprint = '{}'",
                fingerprint(500).as_str()
            )
        )
        .await,
        "1"
    );
}

pub(crate) async fn approving_makes_an_active_member_bound_to_the_key_and_audits_without_content<
    H: Harness,
>() {
    let mut h = H::fresh().await;
    clock_at(&mut h, 7_000);
    let store = h.store();
    let sysop = member(store, "the-sysop").await;
    let bob = member(store, "bob").await;
    let id = stored(store, 1, HANDLE).await;
    store.vouch_request(id, bob.id).await.unwrap();

    let account = store.approve_request(id, sysop.id).await.unwrap();
    assert_eq!(account.username.as_str(), HANDLE);
    assert_eq!(account.state, AccountState::Active);

    // The key signs in as that account, as a plain member (not the waiting room).
    let bound = store
        .account_by_fingerprint(&fingerprint(1))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(bound.id, account.id);
    assert_eq!(
        top_role_of(store, &fingerprint(1)).await,
        Some(Slug::MEMBER)
    );
    // The approver is recorded as the one who admitted them.
    assert_eq!(
        h.texts(&format!(
            "SELECT CAST(invited_by AS TEXT) FROM accounts WHERE id = {}",
            account.id
        ))
        .await,
        [sysop.id.to_string()]
    );
    // The request and its vouches are gone.
    assert_eq!(queue(store).await.requests.len(), 0);
    assert_eq!(count(&h, "invite_request_vouch").await, "0");
    assert_eq!(count(&h, "request_denial").await, "0");

    let rows = all(store).await;
    let created: Vec<_> = rows
        .iter()
        .filter(|r| {
            r.event.action == Action::AccountCreated
                && r.event.target == Target::Account(account.id)
        })
        .collect();
    assert_eq!(created.len(), 1);
    assert_eq!(created[0].event.actor, Actor::Account(sysop.id));
    assert_eq!(
        created[0].event.detail.as_str(),
        format!("via request {id} state active")
    );
    let approved: Vec<_> = rows
        .iter()
        .filter(|r| r.event.action == Action::MemberApproved)
        .collect();
    assert_eq!(approved.len(), 1);
    assert_eq!(approved[0].event.actor, Actor::Account(sysop.id));
    assert_eq!(approved[0].event.target, Target::Account(account.id));
    assert_eq!(approved[0].event.detail.as_str(), format!("request {id}"));
    assert_eq!(approved[0].at.as_raw(), 7_000);
    // Neither the message nor the key is in the log.
    let dump = format!("{rows:?}");
    assert!(!dump.contains(MESSAGE) && !dump.contains(fingerprint(1).as_str()));
    // Approving again finds nothing.
    assert!(matches!(
        store.approve_request(id, sysop.id).await,
        Err(ApproveError::NotFound)
    ));
}

pub(crate) async fn approval_refused_for_a_taken_handle_or_a_bound_key_changes_nothing<
    H: Harness,
>() {
    let h = H::fresh().await;
    let store = h.store();
    let sysop = member(store, "the-sysop").await;
    member(store, "token-name").await;

    // The same handle, and a lookalike of it (case and confusables, D17).
    for (key, handle) in [(1, "token-name"), (2, "Token-Name"), (3, "t0ken-name")] {
        let id = stored(store, key, handle).await;
        let before = all(store).await.len();
        assert!(
            matches!(
                store.approve_request(id, sysop.id).await,
                Err(ApproveError::HandleTaken)
            ),
            "{handle}"
        );
        // The request is still there, nothing was created, no row was written.
        assert!(queue(store).await.requests.iter().any(|r| r.id == id));
        assert_eq!(all(store).await.len(), before);
    }
    assert_eq!(count(&h, "accounts").await, "2");

    // A key that got an account after it asked.
    let id = stored(store, 9, "late-bound").await;
    let other = member(store, "other-account").await;
    store
        .attach_fingerprint(other.id, &fingerprint(9))
        .await
        .unwrap();
    assert!(matches!(
        store.approve_request(id, sysop.id).await,
        Err(ApproveError::KeyHasAccount)
    ));
    assert!(queue(store).await.requests.iter().any(|r| r.id == id));
    assert_eq!(count(&h, "accounts").await, "3");
    // The sysop can still clear it by denying.
    store
        .deny_request(id, sysop.id, DENIAL_COOLDOWN_SECS)
        .await
        .unwrap();
}

pub(crate) async fn denying_deletes_the_request_and_keeps_only_a_hash_and_an_expiry<H: Harness>() {
    let mut h = H::fresh().await;
    clock_at(&mut h, 2_000);
    let store = h.store();
    let moderator = member(store, "the-moderator").await;
    let bob = member(store, "bob").await;
    let id = stored(store, 1, HANDLE).await;
    store.vouch_request(id, bob.id).await.unwrap();

    store
        .deny_request(id, moderator.id, DENIAL_COOLDOWN_SECS)
        .await
        .unwrap();
    assert_eq!(queue(store).await.requests.len(), 0);
    assert_eq!(count(&h, "invite_request").await, "0");
    assert_eq!(count(&h, "invite_request_vouch").await, "0");
    // What remains: a hash and an expiry. Nothing of the request.
    let kept = h
        .texts("SELECT key_hash || '|' || CAST(expires_at AS TEXT) FROM request_denial")
        .await;
    assert_eq!(
        kept,
        [format!(
            "{}|{}",
            cooldown_key(&fingerprint(1)),
            2_000 + DENIAL_COOLDOWN_SECS
        )]
    );
    let hash_table = kept.join("\n");
    assert!(!hash_table.contains(HANDLE) && !hash_table.contains(MESSAGE));
    assert!(!hash_table.contains(fingerprint(1).as_str().trim_start_matches("SHA256:")));
    assert_eq!(count(&h, "accounts").await, "2", "no account was made");

    // The audit row names the request and the decider, and holds no content and no key.
    let rows = all(store).await;
    let denied: Vec<_> = rows
        .iter()
        .filter(|r| r.event.action == Action::MemberDenied)
        .collect();
    assert_eq!(denied.len(), 1);
    assert_eq!(denied[0].event.actor, Actor::Account(moderator.id));
    assert_eq!(denied[0].event.target, Target::Request(id));
    assert_eq!(denied[0].event.detail.as_str(), "");
    let dump = format!("{rows:?}");
    assert!(!dump.contains(HANDLE) && !dump.contains(MESSAGE));
    assert!(!dump.contains(fingerprint(1).as_str()));
    assert!(!dump.contains(&cooldown_key(&fingerprint(1))));
    // Denying again finds nothing and writes nothing.
    let before = rows.len();
    assert!(matches!(
        store
            .deny_request(id, moderator.id, DENIAL_COOLDOWN_SECS)
            .await,
        Err(DecideError::NotFound)
    ));
    assert_eq!(all(store).await.len(), before);
}

pub(crate) async fn a_denied_key_is_turned_away_until_the_cooldown_ends_and_then_nothing_remains<
    H: Harness,
>() {
    let mut h = H::fresh().await;
    let clock = clock_at(&mut h, 10_000);
    let store = h.store();
    let sysop = member(store, "the-sysop").await;
    let id = stored(store, 1, HANDLE).await;
    store
        .deny_request(id, sysop.id, DENIAL_COOLDOWN_SECS)
        .await
        .unwrap();

    let try_again = |s: &H::Store| {
        let s = s.clone();
        async move {
            s.submit_request(&request(1, HANDLE), terms(200))
                .await
                .unwrap()
        }
    };
    // Immediately, and until the last second: turned away, nothing stored.
    assert_eq!(try_again(store).await, Submitted::Refused(Refusal::Cooling));
    clock.set(10_000 + DENIAL_COOLDOWN_SECS - 1);
    assert_eq!(try_again(store).await, Submitted::Refused(Refusal::Cooling));
    assert_eq!(count(&h, "invite_request").await, "0");
    assert_eq!(count(&h, "request_denial").await, "1");
    // Another key is not affected by it.
    assert!(matches!(
        store
            .submit_request(&request(2, "another-one"), terms(200))
            .await
            .unwrap(),
        Submitted::Stored(_)
    ));

    // At the expiry the cooldown is purged by the next submission, and the key may ask again.
    clock.set(10_000 + DENIAL_COOLDOWN_SECS);
    assert!(matches!(try_again(store).await, Submitted::Stored(_)));
    assert_eq!(
        count(&h, "request_denial").await,
        "0",
        "nothing of the denial remains"
    );
}

pub(crate) async fn requests_expire_after_the_ttl_and_the_purge_removes_every_trace<H: Harness>() {
    let mut h = H::fresh().await;
    let clock = clock_at(&mut h, 1_000);
    let store = h.store();
    let sysop = member(store, "the-sysop").await;
    let bob = member(store, "bob").await;
    let old = stored(store, 1, "old-one").await;
    store.vouch_request(old, bob.id).await.unwrap();
    clock.set(1_000 + PENDING_TTL_SECS - 1);
    let young = stored(store, 2, "young-one").await;

    // One second before the first expires, both are listed.
    assert_eq!(queue(store).await.requests.len(), 2);
    // At the expiry the first is gone from the list and from the disk, with its vouch.
    clock.set(1_000 + PENDING_TTL_SECS);
    let listed = queue(store).await.requests;
    assert_eq!(listed.iter().map(|r| r.id).collect::<Vec<_>>(), [young]);
    assert_eq!(count(&h, "invite_request").await, "1");
    assert_eq!(count(&h, "invite_request_vouch").await, "0");
    // Expired requests cannot be decided or vouched for, even before anything purged them.
    assert!(matches!(
        store.approve_request(old, sysop.id).await,
        Err(ApproveError::NotFound)
    ));
    assert!(matches!(
        store.vouch_request(old, sysop.id).await,
        Err(VouchError::NotFound)
    ));
    // The key that asked may ask again at once: an expired request is not a refusal.
    assert!(matches!(
        store
            .submit_request(&request(1, "old-one"), terms(200))
            .await
            .unwrap(),
        Submitted::Stored(_)
    ));

    // The explicit purge reports what it removed, and a denial's cooldown goes the same way.
    store.deny_request(young, sysop.id, 50).await.unwrap();
    clock.set(1_000 + PENDING_TTL_SECS + 49);
    assert_eq!(store.purge_expired().await.unwrap().cooldowns, 0);
    clock.set(1_000 + PENDING_TTL_SECS + 50);
    let purged = store.purge_expired().await.unwrap();
    assert_eq!((purged.requests, purged.cooldowns), (0, 1));
    assert_eq!(count(&h, "request_denial").await, "0");
    clock.set(1_000 + 2 * PENDING_TTL_SECS);
    assert_eq!(store.purge_expired().await.unwrap().requests, 1);
    assert_eq!(count(&h, "invite_request").await, "0");
}

pub(crate) async fn vouching_adds_a_name_once_and_the_queue_shows_active_vouchers_only<
    H: Harness,
>() {
    let mut h = H::fresh().await;
    let clock = clock_at(&mut h, 3_000);
    let store = h.store();
    let bob = member(store, "bob").await;
    let carol = member(store, "carol").await;
    let dave = member(store, "dave").await;
    let id = stored(store, 1, HANDLE).await;

    store.vouch_request(id, bob.id).await.unwrap();
    clock.set(3_100);
    store.vouch_request(id, carol.id).await.unwrap();
    store.vouch_request(id, dave.id).await.unwrap();
    // A member cannot vouch twice, and the refusal writes nothing.
    let before = count_of(&all(store).await, Action::MemberVouched);
    assert_eq!(before, 3);
    assert!(matches!(
        store.vouch_request(id, bob.id).await,
        Err(VouchError::AlreadyVouched)
    ));
    assert_eq!(count_of(&all(store).await, Action::MemberVouched), before);
    assert_eq!(count(&h, "invite_request_vouch").await, "3");
    // No such request.
    assert!(matches!(
        store.vouch_request(RequestId::from_raw(999), bob.id).await,
        Err(VouchError::NotFound)
    ));

    // The sysop sees the names, oldest vouch first.
    let vouchers: Vec<_> = queue(store).await.requests[0]
        .vouchers
        .iter()
        .map(|v| v.handle.as_str().to_owned())
        .collect();
    assert_eq!(vouchers, ["bob", "carol", "dave"]);

    // A suspended member's name is not shown; a deleted account's vouch is gone.
    h.raw(&format!(
        "UPDATE accounts SET state = 'suspended' WHERE id = {}",
        carol.id
    ))
    .await
    .unwrap();
    h.raw(&format!("DELETE FROM accounts WHERE id = {}", dave.id))
        .await
        .unwrap();
    let vouchers: Vec<_> = queue(store).await.requests[0]
        .vouchers
        .iter()
        .map(|v| v.handle.as_str().to_owned())
        .collect();
    assert_eq!(vouchers, ["bob"]);
    assert_eq!(count(&h, "invite_request").await, "1", "the request stays");

    // Each vouch is audited by the voucher, against the request, with no content.
    let rows = all(store).await;
    let vouched: Vec<_> = rows
        .iter()
        .filter(|r| r.event.action == Action::MemberVouched)
        .collect();
    assert_eq!(vouched.len(), 3);
    assert!(
        vouched
            .iter()
            .all(|r| r.event.target == Target::Request(id))
    );
    assert!(vouched.iter().all(|r| r.event.detail.as_str().is_empty()));
    assert_eq!(vouched[2].event.actor, Actor::Account(bob.id));
    assert_eq!(vouched[2].at.as_raw(), 3_000);
}

pub(crate) async fn the_queue_lists_oldest_first_and_a_limit_cuts_the_newest<H: Harness>() {
    let mut h = H::fresh().await;
    let clock = clock_at(&mut h, 100);
    let store = h.store();
    let mut ids = Vec::new();
    for n in 1..=5_u32 {
        clock.set(100 * i64::from(n));
        ids.push(stored(store, n, &format!("person{n}")).await);
    }
    let listed = store.pending_requests(3).await.unwrap();
    assert_eq!(
        listed.requests.iter().map(|r| r.id).collect::<Vec<_>>(),
        ids[..3]
    );
    assert_eq!(listed.requests[0].created_at.as_raw(), 100);
    assert_eq!(listed.now.as_raw(), 500);
    assert_eq!(store.pending_requests(100).await.unwrap().requests.len(), 5);
}

pub(crate) async fn racing_decisions_on_one_request_have_exactly_one_winner<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let sysop = member(store, "the-sysop").await;
    for round in 0..3_u32 {
        let id = stored(store, 10 + round, &format!("racer{round}")).await;
        let mut approvals = Vec::new();
        for _ in 0..5 {
            let store = store.clone();
            approvals.push(tokio::spawn(async move {
                store.approve_request(id, sysop.id).await.map(|_| ())
            }));
        }
        let denial = {
            let store = store.clone();
            tokio::spawn(async move {
                store
                    .deny_request(id, sysop.id, DENIAL_COOLDOWN_SECS)
                    .await
                    .map_err(|error| format!("{error:?}"))
            })
        };
        let mut winners = 0;
        for task in approvals {
            match task.await.unwrap() {
                Ok(()) => winners += 1,
                Err(error) => assert!(matches!(error, ApproveError::NotFound), "{error:?}"),
            }
        }
        match denial.await.unwrap() {
            Ok(()) => winners += 1,
            Err(error) => assert!(error.contains("NotFound"), "{error}"),
        }
        assert_eq!(winners, 1, "round {round}");
    }
    // Every request was decided once: each left either one account or one cooldown, never both.
    let accounts: usize = count(&h, "accounts").await.parse().unwrap();
    let cooldowns: usize = count(&h, "request_denial").await.parse().unwrap();
    assert_eq!((accounts - 1) + cooldowns, 3);
    assert_eq!(count(&h, "invite_request").await, "0");
}

pub(crate) async fn queue_writes_never_land_without_their_audit_row<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let sysop = member(store, "the-sysop").await;
    let bob = member(store, "bob").await;
    let to_approve = stored(store, 1, "approve-me").await;
    let to_deny = stored(store, 2, "deny-me").await;
    let to_vouch = stored(store, 3, "vouch-me").await;
    h.break_audit_inserts().await;

    assert!(matches!(
        store.approve_request(to_approve, sysop.id).await,
        Err(ApproveError::Store(_))
    ));
    assert!(matches!(
        store
            .deny_request(to_deny, sysop.id, DENIAL_COOLDOWN_SECS)
            .await,
        Err(DecideError::Store(_))
    ));
    assert!(matches!(
        store.vouch_request(to_vouch, bob.id).await,
        Err(VouchError::Store(_))
    ));
    // Nothing moved: all three requests are there, no account, no cooldown, no vouch.
    assert_eq!(count(&h, "invite_request").await, "3");
    assert_eq!(count(&h, "accounts").await, "2");
    assert_eq!(count(&h, "account_keys").await, "0");
    assert_eq!(count(&h, "request_denial").await, "0");
    assert_eq!(count(&h, "invite_request_vouch").await, "0");
    // A submission writes no audit row, so it still works with the log broken.
    assert!(matches!(
        store
            .submit_request(&request(4, "still-works"), terms(200))
            .await
            .unwrap(),
        Submitted::Stored(_)
    ));
}

pub(crate) async fn the_queue_survives_a_reopen<H: Harness>() {
    let mut h = H::fresh().await;
    clock_at(&mut h, 4_000);
    let id = stored(h.store(), 1, HANDLE).await;
    let sysop = member(h.store(), "the-sysop").await;
    h.store().vouch_request(id, sysop.id).await.unwrap();
    let denied = stored(h.store(), 2, "denied-one").await;
    h.store()
        .deny_request(denied, sysop.id, DENIAL_COOLDOWN_SECS)
        .await
        .unwrap();
    h.reopen().await;
    clock_at(&mut h, 4_001);
    let listed = queue(h.store()).await.requests;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].handle.as_str(), HANDLE);
    assert_eq!(listed[0].vouchers.len(), 1);
    assert_eq!(
        h.store()
            .submit_request(&request(2, "denied-one"), terms(200))
            .await
            .unwrap(),
        Submitted::Refused(Refusal::Cooling)
    );
}

pub(crate) async fn a_broken_store_errors_instead_of_answering<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let sysop = member(store, "the-sysop").await;
    let id = stored(store, 1, HANDLE).await;
    h.close().await;
    assert!(
        store
            .submit_request(&request(2, "after-close"), terms(200))
            .await
            .is_err()
    );
    assert!(store.pending_requests(10).await.is_err());
    assert!(matches!(
        store.approve_request(id, sysop.id).await,
        Err(ApproveError::Store(StoreError::Backend(_)))
    ));
    assert!(matches!(
        store.deny_request(id, sysop.id, 10).await,
        Err(DecideError::Store(_))
    ));
    assert!(matches!(
        store.vouch_request(id, sysop.id).await,
        Err(VouchError::Store(_))
    ));
    assert!(store.purge_expired().await.is_err());
}
