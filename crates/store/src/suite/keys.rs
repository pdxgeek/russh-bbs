//! Keys, device-link and recovery codes and key notices against any backend (FIX-M3-11, D97):
//! the last way in is never removed (even by two removals at once), a code works once and
//! expires to the second, a key that has an account rolls a redemption back without using the
//! code up, the hourly limit holds, a newer code voids the last, a recovery code binds a key and
//! replaces the password in one step, nothing readable is stored, and every change leaves its
//! audit row and its notice in the same transaction.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::sync::Arc;

use russh_bbs_core::access::{Permission, Slug};
use russh_bbs_core::audit::{
    Action, AuditEntry, AuditQuery, AuditStore, FixedClock, events, known_events,
};
use russh_bbs_core::keys::{
    CodeKind, IssueError, KeyStore, LoginCode, NewCode, NoticeKind, RedeemError, RedeemRefusal,
    RemoveOutcome,
};
use russh_bbs_core::passwords::{PasswordStore, hash::hash};
use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::store::{Account, AccountId, AccountState, AccountStore, NewAccount, StoreError};

use super::{Harness, fingerprint};

const T0: i64 = 1_800_000_000;

fn clock_at(h: &mut impl Harness, at: i64) -> Arc<FixedClock> {
    let clock = Arc::new(FixedClock::new(at));
    h.set_clock(clock.clone());
    clock
}

async fn account<S: AccountStore>(store: &S, text: &str, role: Slug) -> Account {
    store
        .create_account(NewAccount {
            username: sanitize_username(text).unwrap(),
            role,
            state: AccountState::Active,
        })
        .await
        .unwrap()
}

async fn member<S: AccountStore>(store: &S, text: &str) -> Account {
    account(store, text, Slug::MEMBER).await
}

async fn with_keys<S: AccountStore>(store: &S, text: &str, keys: &[u32]) -> Account {
    let acct = member(store, text).await;
    for key in keys {
        store
            .attach_fingerprint(acct.id, &fingerprint(*key))
            .await
            .unwrap();
    }
    acct
}

async fn with_password<S: PasswordStore>(store: &S, acct: &Account) {
    store
        .set_hash(
            acct.id,
            Some(&hash("a long enough passphrase").unwrap()),
            &events::password_set(acct.id, "test"),
        )
        .await
        .unwrap();
}

fn code(kind: CodeKind, seed: u8) -> LoginCode {
    LoginCode::from_bytes(kind, &[seed; 16])
}

fn new_code(kind: CodeKind, target: &Account, issuer: &Account, code: &LoginCode) -> NewCode {
    NewCode {
        kind,
        target: target.id,
        issuer: issuer.id,
        hash: code.hash(),
        ttl_secs: kind.ttl_secs(),
        per_hour: None,
    }
}

async fn issue<S: KeyStore>(
    store: &S,
    kind: CodeKind,
    target: &Account,
    issuer: &Account,
    code: &LoginCode,
) -> Result<russh_bbs_core::store::UnixSeconds, IssueError> {
    let audit = match kind {
        CodeKind::Link => events::device_code_issued(target.id),
        CodeKind::Recovery => events::recovery_code_issued(issuer.id, target.id),
    };
    store
        .issue_code(&new_code(kind, target, issuer, code), &audit)
        .await
}

async fn rows<S: AuditStore>(store: &S) -> Vec<AuditEntry> {
    known_events(store.page(&AuditQuery::default()).await.unwrap())
}

fn count_of(rows: &[AuditEntry], action: Action) -> usize {
    rows.iter().filter(|r| r.event.action == action).count()
}

async fn count<H: Harness>(h: &H, from_where: &str) -> String {
    h.texts(&format!("SELECT CAST(count(*) AS TEXT) FROM {from_where}"))
        .await
        .remove(0)
}

fn refused(error: &RedeemError) -> Option<RedeemRefusal> {
    match error {
        RedeemError::Refused(why) => Some(*why),
        RedeemError::Store(_) => None,
    }
}

pub(crate) async fn keys_are_listed_added_and_removed_with_audit_rows_and_notices<H: Harness>() {
    let mut h = H::fresh().await;
    let clock = clock_at(&mut h, T0);
    let store = h.store();
    let alice = with_keys(store, "alice", &[1]).await;
    clock.set(T0 + 10);
    let added = fingerprint(2);
    store
        .add_key(alice.id, &added, &events::key_added(alice.id, &added))
        .await
        .unwrap();
    let listed = store.list_keys(alice.id).await.unwrap();
    assert_eq!(listed.len(), 2);
    assert!(listed.iter().any(|k| k.fingerprint == added));
    assert!(listed.iter().all(|k| k.last_used_at.is_none()));
    // A key bound by someone else's account is not in this list.
    let bob = with_keys(store, "bob", &[3]).await;
    assert_eq!(store.list_keys(bob.id).await.unwrap().len(), 1);

    clock.set(T0 + 20);
    let outcome = store
        .remove_key(alice.id, &fingerprint(1), &events::key_removed(alice.id, &fingerprint(1)))
        .await
        .unwrap();
    assert_eq!(outcome, RemoveOutcome::Removed);
    assert_eq!(store.list_keys(alice.id).await.unwrap().len(), 1);

    let log = rows(store).await;
    assert_eq!(count_of(&log, Action::KeyAdded), 1);
    assert_eq!(count_of(&log, Action::KeyRemoved), 1);
    // The member is told, in the order it happened, and only once.
    let told = store.take_notices(alice.id).await.unwrap();
    let kinds: Vec<_> = told.iter().map(|n| n.kind).collect();
    assert_eq!(kinds, [NoticeKind::KeyAdded, NoticeKind::KeyRemoved]);
    assert_eq!(told[0].at.as_raw(), T0 + 10);
    assert_eq!(told[1].fingerprint.as_ref(), Some(&fingerprint(1)));
    assert!(store.take_notices(alice.id).await.unwrap().is_empty());
    // Another member's notices are theirs alone.
    assert!(store.take_notices(bob.id).await.unwrap().is_empty());
}

pub(crate) async fn the_last_way_in_is_never_removed<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let alice = with_keys(store, "alice", &[1]).await;
    let remove = |fp: u32| {
        let fp = fingerprint(fp);
        let audit = events::key_removed(alice.id, &fp);
        async move { store.remove_key(alice.id, &fp, &audit).await.unwrap() }
    };
    // One key, no password: it is the only way in.
    assert_eq!(remove(1).await, RemoveOutcome::LastWayIn);
    assert_eq!(store.list_keys(alice.id).await.unwrap().len(), 1);
    // A key the account does not have is "not found", not "last way in".
    assert_eq!(remove(9).await, RemoveOutcome::NotFound);
    let log = rows(store).await;
    assert_eq!(count_of(&log, Action::KeyRemoved), 0);
    assert!(store.take_notices(alice.id).await.unwrap().is_empty());

    // With a password the key can go.
    with_password(store, &alice).await;
    assert_eq!(remove(1).await, RemoveOutcome::Removed);
    assert!(store.list_keys(alice.id).await.unwrap().is_empty());

    // Two keys, no password: one goes, the last stays.
    let carol = with_keys(store, "carol", &[5, 6]).await;
    let first = fingerprint(5);
    let second = fingerprint(6);
    assert_eq!(
        store
            .remove_key(carol.id, &first, &events::key_removed(carol.id, &first))
            .await
            .unwrap(),
        RemoveOutcome::Removed
    );
    assert_eq!(
        store
            .remove_key(carol.id, &second, &events::key_removed(carol.id, &second))
            .await
            .unwrap(),
        RemoveOutcome::LastWayIn
    );
    // Another account's key is never removable through this one.
    let dave = with_keys(store, "dave", &[7]).await;
    with_password(store, &dave).await;
    assert_eq!(
        store
            .remove_key(dave.id, &second, &events::key_removed(dave.id, &second))
            .await
            .unwrap(),
        RemoveOutcome::NotFound
    );
    assert_eq!(store.list_keys(carol.id).await.unwrap().len(), 1);
}

pub(crate) async fn two_removals_racing_for_the_last_two_keys_cannot_both_succeed<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store().clone();
    for round in 0..8_u32 {
        let acct = with_keys(&store, &format!("racer{round}"), &[100 + round * 2, 101 + round * 2])
            .await;
        let (a, b) = (fingerprint(100 + round * 2), fingerprint(101 + round * 2));
        let run = |fp: russh_bbs_core::access::Fingerprint| {
            let store = store.clone();
            tokio::spawn(async move {
                let audit = events::key_removed(acct.id, &fp);
                store.remove_key(acct.id, &fp, &audit).await
            })
        };
        let (x, y) = (run(a), run(b));
        let results = [x.await.unwrap(), y.await.unwrap()];
        // SQLite may report a busy writer as an error; it must never report two removals.
        let removed = results
            .iter()
            .filter(|r| matches!(r, Ok(RemoveOutcome::Removed)))
            .count();
        assert!(removed <= 1, "both removals succeeded in round {round}");
        assert!(
            !store.list_keys(acct.id).await.unwrap().is_empty(),
            "the account lost its last key in round {round}"
        );
    }
}

pub(crate) async fn a_key_that_has_an_account_cannot_be_added_and_nothing_is_written<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let alice = with_keys(store, "alice", &[1]).await;
    let bob = with_keys(store, "bob", &[2]).await;
    for (who, key) in [(&bob, 1_u32), (&alice, 1), (&alice, 2)] {
        let fp = fingerprint(key);
        let error = store
            .add_key(who.id, &fp, &events::key_added(who.id, &fp))
            .await
            .unwrap_err();
        assert!(matches!(error, StoreError::FingerprintTaken));
    }
    assert_eq!(store.list_keys(alice.id).await.unwrap().len(), 1);
    assert_eq!(store.list_keys(bob.id).await.unwrap().len(), 1);
    assert_eq!(count_of(&rows(store).await, Action::KeyAdded), 0);
    assert!(store.take_notices(alice.id).await.unwrap().is_empty());
    let missing = store
        .add_key(
            AccountId::from_raw(9_999),
            &fingerprint(3),
            &events::key_added(alice.id, &fingerprint(3)),
        )
        .await;
    assert!(missing.is_err());
}

pub(crate) async fn a_link_code_binds_a_key_once_and_expires_to_the_second<H: Harness>() {
    let mut h = H::fresh().await;
    let clock = clock_at(&mut h, T0);
    let store = h.store();
    let alice = with_keys(store, "alice", &[1]).await;
    let c = code(CodeKind::Link, 4);
    let expires = issue(store, CodeKind::Link, &alice, &alice, &c).await.unwrap();
    assert_eq!(expires.as_raw(), T0 + 600);

    // A wrong code is refused and nothing changes.
    let wrong = code(CodeKind::Link, 5);
    let error = store
        .redeem_link_code(&wrong.hash(), &fingerprint(20))
        .await
        .unwrap_err();
    assert_eq!(refused(&error), Some(RedeemRefusal::Code));

    // A key that belongs to an account refuses the redemption and does not use the code up.
    let taken = store
        .redeem_link_code(&c.hash(), &fingerprint(1))
        .await
        .unwrap_err();
    assert_eq!(refused(&taken), Some(RedeemRefusal::Key));
    assert_eq!(count(&h, "login_code WHERE redeemed_at IS NOT NULL").await, "0");

    // One second before expiry it works; the key is bound to the code's account.
    clock.set(T0 + 599);
    let bound = store
        .redeem_link_code(&c.hash(), &fingerprint(20))
        .await
        .unwrap();
    assert_eq!(bound.id, alice.id);
    assert_eq!(store.list_keys(alice.id).await.unwrap().len(), 2);
    // Single use, whoever asks.
    let again = store
        .redeem_link_code(&c.hash(), &fingerprint(21))
        .await
        .unwrap_err();
    assert_eq!(refused(&again), Some(RedeemRefusal::Code));

    // An unused code is dead at its expiry second.
    let late = code(CodeKind::Link, 6);
    issue(store, CodeKind::Link, &alice, &alice, &late).await.unwrap();
    clock.set(T0 + 599 + 600);
    let expired = store
        .redeem_link_code(&late.hash(), &fingerprint(22))
        .await
        .unwrap_err();
    assert_eq!(refused(&expired), Some(RedeemRefusal::Code));

    let log = rows(store).await;
    assert_eq!(count_of(&log, Action::DeviceCodeIssued), 2);
    assert_eq!(count_of(&log, Action::DeviceCodeRedeemed), 1);
    let kinds: Vec<_> = store
        .take_notices(alice.id)
        .await
        .unwrap()
        .iter()
        .map(|n| n.kind)
        .collect();
    assert_eq!(kinds, [NoticeKind::DeviceLinked]);
}

pub(crate) async fn a_code_is_used_once_when_two_redeem_it_at_the_same_time<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store().clone();
    let alice = with_keys(&store, "alice", &[1]).await;
    let c = code(CodeKind::Link, 8);
    issue(&store, CodeKind::Link, &alice, &alice, &c).await.unwrap();
    let hash = c.hash();
    let tasks: Vec<_> = (0..4_u32)
        .map(|n| {
            let store = store.clone();
            tokio::spawn(async move { store.redeem_link_code(&hash, &fingerprint(30 + n)).await })
        })
        .collect();
    let mut won = 0;
    for task in tasks {
        if task.await.unwrap().is_ok() {
            won += 1;
        }
    }
    assert_eq!(won, 1);
    assert_eq!(store.list_keys(alice.id).await.unwrap().len(), 2);
}

pub(crate) async fn the_hourly_limit_holds_and_a_newer_code_voids_the_last<H: Harness>() {
    let mut h = H::fresh().await;
    let clock = clock_at(&mut h, T0);
    let store = h.store();
    let alice = with_keys(store, "alice", &[1]).await;
    let bob = with_keys(store, "bob", &[2]).await;
    let limited = |seed: u8, who: &Account| NewCode {
        per_hour: Some(2),
        ..new_code(CodeKind::Link, who, who, &code(CodeKind::Link, seed))
    };
    let audit = |who: &Account| events::device_code_issued(who.id);
    store.issue_code(&limited(1, &alice), &audit(&alice)).await.unwrap();
    clock.set(T0 + 100);
    store.issue_code(&limited(2, &alice), &audit(&alice)).await.unwrap();
    clock.set(T0 + 200);
    let third = store
        .issue_code(&limited(3, &alice), &audit(&alice))
        .await
        .unwrap_err();
    let IssueError::RateLimited { retry_at } = third else {
        panic!("the third code in an hour is limited");
    };
    // The oldest stops counting an hour after it was made.
    assert_eq!(retry_at.as_raw(), T0 + 3_600);
    // The refusal wrote nothing: no row, no audit row, and the older codes still stand.
    assert_eq!(count(&h, "login_code").await, "2");
    assert_eq!(count_of(&rows(store).await, Action::DeviceCodeIssued), 2);
    // The limit is per account.
    store.issue_code(&limited(4, &bob), &audit(&bob)).await.unwrap();
    clock.set(T0 + 3_601);
    store.issue_code(&limited(5, &alice), &audit(&alice)).await.unwrap();

    // The second code replaced the first: the first no longer redeems, the live one does.
    let first = store
        .redeem_link_code(&code(CodeKind::Link, 1).hash(), &fingerprint(40))
        .await
        .unwrap_err();
    assert_eq!(refused(&first), Some(RedeemRefusal::Code));
    let voided_second = store
        .redeem_link_code(&code(CodeKind::Link, 2).hash(), &fingerprint(41))
        .await
        .unwrap_err();
    assert_eq!(refused(&voided_second), Some(RedeemRefusal::Code));
    store
        .redeem_link_code(&code(CodeKind::Link, 5).hash(), &fingerprint(42))
        .await
        .unwrap();

    // Not active: no code.
    let gone = account(store, "gone", Slug::MEMBER).await;
    h.raw(&format!("UPDATE accounts SET state = 'suspended' WHERE id = {}", gone.id))
        .await
        .unwrap();
    let none = store
        .issue_code(&limited(6, &gone), &audit(&gone))
        .await
        .unwrap_err();
    assert!(matches!(none, IssueError::NoAccount));
}

pub(crate) async fn a_recovery_code_binds_a_key_and_replaces_the_password_in_one_step<H: Harness>() {
    let mut h = H::fresh().await;
    let clock = clock_at(&mut h, T0);
    let store = h.store();
    let staff = account(store, "boss", Slug::SYSOP).await;
    let alice = with_keys(store, "alice", &[1]).await;
    with_password(store, &alice).await;
    let before = store.stored_hash(alice.id).await.unwrap().unwrap();
    let c = code(CodeKind::Recovery, 9);
    let expires = issue(store, CodeKind::Recovery, &alice, &staff, &c).await.unwrap();
    assert_eq!(expires.as_raw(), T0 + 86_400);

    // The kinds do not cross: a recovery code is not a link code and the reverse.
    let as_link = store
        .redeem_link_code(&c.hash(), &fingerprint(50))
        .await
        .unwrap_err();
    assert_eq!(refused(&as_link), Some(RedeemRefusal::Code));
    let link = code(CodeKind::Link, 9);
    issue(store, CodeKind::Link, &alice, &alice, &link).await.unwrap();
    let as_recovery = store
        .redeem_recovery_code(&link.hash(), &fingerprint(51), &hash("another long passphrase").unwrap())
        .await
        .unwrap_err();
    assert_eq!(refused(&as_recovery), Some(RedeemRefusal::Code));

    // A key that has an account: refused, the password is untouched, the code survives.
    let new_hash = hash("another long passphrase").unwrap();
    let taken = store
        .redeem_recovery_code(&c.hash(), &fingerprint(1), &new_hash)
        .await
        .unwrap_err();
    assert_eq!(refused(&taken), Some(RedeemRefusal::Key));
    assert_eq!(store.stored_hash(alice.id).await.unwrap().unwrap(), before);

    clock.set(T0 + 86_399);
    let done = store
        .redeem_recovery_code(&c.hash(), &fingerprint(52), &new_hash)
        .await
        .unwrap();
    assert_eq!(done.id, alice.id);
    assert_eq!(store.stored_hash(alice.id).await.unwrap().unwrap(), new_hash);
    assert_eq!(store.list_keys(alice.id).await.unwrap().len(), 2);
    let again = store
        .redeem_recovery_code(&c.hash(), &fingerprint(53), &new_hash)
        .await
        .unwrap_err();
    assert_eq!(refused(&again), Some(RedeemRefusal::Code));

    let log = rows(store).await;
    assert_eq!(count_of(&log, Action::RecoveryCodeIssued), 1);
    assert_eq!(count_of(&log, Action::RecoveryCodeRedeemed), 1);
    let issued = log
        .iter()
        .find(|r| r.event.action == Action::RecoveryCodeIssued)
        .unwrap();
    assert_eq!(issued.event.actor, russh_bbs_core::audit::Actor::Account(staff.id));
    let notices = store.take_notices(alice.id).await.unwrap();
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0].kind, NoticeKind::Recovered);
}

pub(crate) async fn a_recovery_for_an_expired_or_inactive_account_is_refused<H: Harness>() {
    let mut h = H::fresh().await;
    let clock = clock_at(&mut h, T0);
    let store = h.store();
    let staff = account(store, "boss", Slug::SYSOP).await;
    let alice = with_keys(store, "alice", &[1]).await;
    let c = code(CodeKind::Recovery, 3);
    issue(store, CodeKind::Recovery, &alice, &staff, &c).await.unwrap();
    let new_hash = hash("another long passphrase").unwrap();
    // The day is up at the second.
    clock.set(T0 + 86_400);
    let late = store
        .redeem_recovery_code(&c.hash(), &fingerprint(60), &new_hash)
        .await
        .unwrap_err();
    assert_eq!(refused(&late), Some(RedeemRefusal::Code));
    // Suspended after issue: the code is dead until the account is active again.
    clock.set(T0 + 10);
    let d = code(CodeKind::Recovery, 4);
    issue(store, CodeKind::Recovery, &alice, &staff, &d).await.unwrap();
    h.raw(&format!("UPDATE accounts SET state = 'suspended' WHERE id = {}", alice.id))
        .await
        .unwrap();
    let suspended = store
        .redeem_recovery_code(&d.hash(), &fingerprint(61), &new_hash)
        .await
        .unwrap_err();
    assert_eq!(refused(&suspended), Some(RedeemRefusal::Code));
    assert!(store.stored_hash(alice.id).await.unwrap().is_none());
    assert_eq!(count(&h, "account_keys").await, "1");
}

pub(crate) async fn rank_follows_the_roles<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let sysop = account(store, "boss", Slug::SYSOP).await;
    let deputy = account(store, "deputy", Slug::COSYSOP).await;
    let mod_ = account(store, "helper", Slug::MODERATOR).await;
    let plain = member(store, "plain").await;
    let rank = |a: &Account| {
        let id = a.id;
        async move { store.account_rank(id).await.unwrap() }
    };
    let (s, d, m, p) = (rank(&sysop).await, rank(&deputy).await, rank(&mod_).await, rank(&plain).await);
    assert!(s > d && d > m && m > p && p > 0, "{s} {d} {m} {p}");
    assert_eq!(
        store.active_account("plain").await.unwrap().map(|a| a.id),
        Some(plain.id)
    );
    h.raw(&format!("UPDATE accounts SET state = 'suspended' WHERE id = {}", plain.id))
        .await
        .unwrap();
    assert!(store.active_account("plain").await.unwrap().is_none());
    assert!(store.active_account("nobody").await.unwrap().is_none());
}

pub(crate) async fn key_use_is_remembered_and_a_linked_key_is_told<H: Harness>() {
    let mut h = H::fresh().await;
    let clock = clock_at(&mut h, T0);
    let store = h.store();
    let alice = with_keys(store, "alice", &[1, 2]).await;
    clock.set(T0 + 77);
    store.note_key_use(&fingerprint(2)).await.unwrap();
    // A key nobody has changes nothing and is not an error.
    store.note_key_use(&fingerprint(99)).await.unwrap();
    let listed = store.list_keys(alice.id).await.unwrap();
    let used = |n: u32| {
        listed
            .iter()
            .find(|k| k.fingerprint == fingerprint(n))
            .unwrap()
            .last_used_at
    };
    assert_eq!(used(1), None);
    assert_eq!(used(2).map(|t| t.as_raw()), Some(T0 + 77));

    // The password-linked key (FIX-M3-12) is told at the next sign-in too.
    let key = fingerprint(3);
    store
        .link_key(alice.id, &key, &events::key_linked(alice.id, &key))
        .await
        .unwrap();
    let told = store.take_notices(alice.id).await.unwrap();
    assert_eq!(told.len(), 1);
    assert_eq!(told[0].kind, NoticeKind::KeyLinked);
    assert_eq!(told[0].fingerprint.as_ref(), Some(&key));
}

pub(crate) async fn a_key_change_never_lands_without_its_audit_row<H: Harness>() {
    let mut h = H::fresh().await;
    clock_at(&mut h, T0);
    let alice_id = {
        let store = h.store();
        let alice = with_keys(store, "alice", &[1, 2]).await;
        let c = code(CodeKind::Link, 2);
        issue(store, CodeKind::Link, &alice, &alice, &c).await.unwrap();
        alice.id
    };
    h.break_audit_inserts().await;
    let store = h.store();
    let new_key = fingerprint(3);
    assert!(
        store
            .add_key(alice_id, &new_key, &events::key_added(alice_id, &new_key))
            .await
            .is_err()
    );
    assert!(
        store
            .remove_key(alice_id, &fingerprint(2), &events::key_removed(alice_id, &fingerprint(2)))
            .await
            .is_err()
    );
    assert!(
        store
            .redeem_link_code(&code(CodeKind::Link, 2).hash(), &fingerprint(4))
            .await
            .is_err()
    );
    assert_eq!(store.list_keys(alice_id).await.unwrap().len(), 2);
    assert_eq!(count(&h, "key_notice").await, "0");
    // The failed redemption did not use the code up.
    assert_eq!(count(&h, "login_code WHERE redeemed_at IS NOT NULL").await, "0");
}

pub(crate) async fn only_the_hash_of_a_code_is_stored_and_nothing_secret_is_audited<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let staff = account(store, "boss", Slug::SYSOP).await;
    let alice = with_keys(store, "alice", &[1]).await;
    let link = code(CodeKind::Link, 11);
    let recovery = code(CodeKind::Recovery, 12);
    issue(store, CodeKind::Link, &alice, &alice, &link).await.unwrap();
    issue(store, CodeKind::Recovery, &alice, &staff, &recovery).await.unwrap();
    store
        .redeem_link_code(&link.hash(), &fingerprint(70))
        .await
        .unwrap();
    let stored = h.texts("SELECT code_hash FROM login_code ORDER BY id").await;
    assert_eq!(stored, [link.hash().to_hex(), recovery.hash().to_hex()]);
    let everything = [
        h.texts("SELECT detail FROM audit_log").await,
        h.texts("SELECT COALESCE(target_ref, '') FROM audit_log").await,
        h.texts("SELECT COALESCE(fingerprint, '') || kind FROM key_notice").await,
    ]
    .concat()
    .join("\n");
    for secret in [link.reveal(), recovery.reveal()] {
        let bare = secret.replace('-', "");
        assert!(!everything.contains(secret) && !everything.contains(&bare));
    }
    for hash in &stored {
        assert!(!everything.contains(hash.as_str()));
    }
}

pub(crate) async fn the_roles_hold_the_key_permissions_the_docs_say<H: Harness>() {
    use std::collections::BTreeSet;
    let h = H::fresh().await;
    let store = h.store();
    let holds = |role: Slug| {
        let store = store;
        async move {
            let info = russh_bbs_core::access::AccessStore::role(store, &role)
                .await
                .unwrap()
                .unwrap();
            [Permission::KeysManage, Permission::KeysRedeem, Permission::KeysRecover]
                .into_iter()
                .filter(|p| info.permissions.contains(p))
                .collect::<BTreeSet<_>>()
        }
    };
    let manage: BTreeSet<_> = [Permission::KeysManage].into();
    assert_eq!(holds(Slug::WAITING).await, [Permission::KeysRedeem].into());
    for role in [Slug::MEMBER, Slug::TRUSTED, Slug::MODERATOR] {
        assert_eq!(holds(role).await, manage);
    }
    let staff: BTreeSet<_> = [Permission::KeysManage, Permission::KeysRecover].into();
    assert_eq!(holds(Slug::COSYSOP).await, staff);
    let all: BTreeSet<_> = [
        Permission::KeysManage,
        Permission::KeysRedeem,
        Permission::KeysRecover,
    ]
    .into();
    assert_eq!(holds(Slug::SYSOP).await, all);
}

pub(crate) async fn deleting_an_account_deletes_its_codes_and_notices<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    let alice = with_keys(store, "alice", &[1]).await;
    let c = code(CodeKind::Link, 13);
    issue(store, CodeKind::Link, &alice, &alice, &c).await.unwrap();
    let key = fingerprint(2);
    store
        .add_key(alice.id, &key, &events::key_added(alice.id, &key))
        .await
        .unwrap();
    h.raw(&format!("DELETE FROM accounts WHERE id = {}", alice.id))
        .await
        .unwrap();
    assert_eq!(count(&h, "login_code").await, "0");
    assert_eq!(count(&h, "key_notice").await, "0");
}

pub(crate) async fn keys_and_codes_survive_a_reopen<H: Harness>() {
    let mut h = H::fresh().await;
    let alice = with_keys(h.store(), "alice", &[1]).await;
    let c = code(CodeKind::Link, 14);
    issue(h.store(), CodeKind::Link, &alice, &alice, &c).await.unwrap();
    let key = fingerprint(2);
    h.store()
        .add_key(alice.id, &key, &events::key_added(alice.id, &key))
        .await
        .unwrap();
    h.reopen().await;
    let store = h.store();
    assert_eq!(store.list_keys(alice.id).await.unwrap().len(), 2);
    store
        .redeem_link_code(&c.hash(), &fingerprint(80))
        .await
        .unwrap();
    assert_eq!(store.take_notices(alice.id).await.unwrap().len(), 2);
}

pub(crate) async fn a_broken_store_errors_instead_of_pretending_there_is_nothing<H: Harness>() {
    let h = H::fresh().await;
    let alice = with_keys(h.store(), "alice", &[1, 2]).await;
    h.close().await;
    let store = h.store();
    assert!(store.list_keys(alice.id).await.is_err());
    assert!(store.take_notices(alice.id).await.is_err());
    assert!(store.account_rank(alice.id).await.is_err());
    assert!(store.active_account("alice").await.is_err());
}
