//! Registration modes `apply` and `open` against any backend (M2-G, D73, D74): `open` makes an
//! active account with no inviter inside one transaction that also counts it against the UTC
//! day's cap; the cap holds under concurrency; the first refusal of a day writes one audit
//! row and the next day starts fresh (an injected clock); a refusal leaves no trace; and an
//! application is a queue row of its own kind with one audit row and no content.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::sync::Arc;

use russh_bbs_core::access::Slug;
use russh_bbs_core::audit::{
    Action, Actor, AuditEntry, AuditQuery, AuditStore, FixedClock, Target,
};
use russh_bbs_core::limits::Day;
use russh_bbs_core::queue::{
    Message, NewRequest, PENDING_TTL_SECS, QueueStore, RequestKind, SubmitTerms, Submitted,
};
use russh_bbs_core::registration::{DailyCap, OpenError, OpenRefusal, RegistrationStore};
use russh_bbs_core::sanitize::sanitize_username;
use russh_bbs_core::store::{
    AccountState, AccountStore, NewAccount, RegistrationMode, SettingKey, SettingsStore as _,
    UnixSeconds, set_registration_mode,
};

use super::{Harness, fingerprint, top_role_of};

/// The first second of day 20,000 (UTC).
const DAY: i64 = 20_000 * 86_400;

fn clock_at(h: &mut impl Harness, at: i64) -> Arc<FixedClock> {
    let clock = Arc::new(FixedClock::new(at));
    h.set_clock(clock.clone());
    clock
}

fn name(text: &str) -> russh_bbs_core::sanitize::Username {
    sanitize_username(text).unwrap()
}

async fn all<S: AuditStore>(store: &S) -> Vec<AuditEntry> {
    russh_bbs_core::audit::known_events(store.page(&AuditQuery::default()).await.unwrap())
}

fn count_of(rows: &[AuditEntry], action: Action) -> usize {
    rows.iter().filter(|r| r.event.action == action).count()
}

async fn count<H: Harness>(h: &H, from_where: &str) -> String {
    h.texts(&format!("SELECT CAST(count(*) AS TEXT) FROM {from_where}"))
        .await
        .remove(0)
}

async fn opened_today<H: Harness>(h: &H, day: i64) -> String {
    h.texts(&format!(
        "SELECT CAST(opened AS TEXT) FROM registration_day WHERE day = {day}"
    ))
    .await
    .pop()
    .unwrap_or_else(|| "none".to_owned())
}

pub(crate) async fn open_makes_an_active_account_with_no_inviter_and_audits_it<H: Harness>() {
    let mut h = H::fresh().await;
    clock_at(&mut h, DAY + 100);
    let store = h.store();
    let made = store
        .open_account(name("newcomer"), &fingerprint(1), DailyCap::Limited(10))
        .await
        .unwrap();
    assert_eq!(made.account.username.as_str(), "newcomer");
    assert_eq!(made.account.state, AccountState::Active);

    // Bound to the key, a plain member, nobody vouched.
    let bound = store
        .account_by_fingerprint(&fingerprint(1))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(bound.id, made.account.id);
    assert_eq!(
        top_role_of(store, &fingerprint(1)).await,
        Some(Slug::MEMBER)
    );
    assert_eq!(
        h.texts(&format!(
            "SELECT CASE WHEN invited_by IS NULL THEN 'null' ELSE 'set' END \
             FROM accounts WHERE id = {}",
            made.account.id
        ))
        .await,
        ["null"]
    );
    // One audit row, by the board, naming the account; no key and no inviter.
    let rows = all(store).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].event.action, Action::AccountCreated);
    assert_eq!(rows[0].event.actor, Actor::System);
    assert_eq!(rows[0].event.target, Target::Account(made.account.id));
    assert_eq!(rows[0].event.detail.as_str(), "via open state active");
    assert!(!format!("{rows:?}").contains(fingerprint(1).as_str()));
    assert_eq!(rows[0].at.as_raw(), DAY + 100);
    // The day's counter has one.
    assert_eq!(
        opened_today(&h, Day::of(UnixSeconds::from_raw(DAY)).as_raw()).await,
        "1"
    );
}

pub(crate) async fn the_cap_degrades_open_for_the_rest_of_the_utc_day_and_the_next_day_starts_fresh<
    H: Harness,
>() {
    let mut h = H::fresh().await;
    let clock = clock_at(&mut h, DAY + 10);
    let store = h.store();
    let cap = DailyCap::Limited(2);
    let day = Day::of(UnixSeconds::from_raw(DAY)).as_raw();

    assert!(!store.open_cap_reached(cap).await.unwrap());
    for n in 1..=2_u32 {
        store
            .open_account(name(&format!("member{n}")), &fingerprint(n), cap)
            .await
            .unwrap();
    }
    assert!(store.open_cap_reached(cap).await.unwrap());
    assert!(!store.open_cap_reached(DailyCap::Unlimited).await.unwrap());
    assert!(!store.open_cap_reached(DailyCap::Limited(3)).await.unwrap());
    // The account that took the last place wrote the day's one audit row, before anyone was
    // turned away.
    assert_eq!(count_of(&all(store).await, Action::RegistrationCapped), 1);

    // Everyone after that is refused, whoever they are, and no account is made.
    for n in 3..=6_u32 {
        let refused = store
            .open_account(name(&format!("member{n}")), &fingerprint(n), cap)
            .await;
        assert!(matches!(refused, Err(OpenError::CapReached)), "{refused:?}");
    }
    assert_eq!(count(&h, "accounts").await, "2");
    assert_eq!(opened_today(&h, day).await, "2");
    // However many were turned away, the day has its one audit row, by the board, with no
    // person in it.
    let rows = all(store).await;
    assert_eq!(count_of(&rows, Action::RegistrationCapped), 1);
    let capped = rows
        .iter()
        .find(|r| r.event.action == Action::RegistrationCapped)
        .unwrap();
    assert_eq!(capped.event.actor, Actor::System);
    assert_eq!(capped.event.target, Target::None);
    assert_eq!(capped.event.detail.as_str(), format!("day {day} cap 2"));

    // Raising the cap the same day lets the next one in (the sysop's choice).
    store
        .open_account(name("member7"), &fingerprint(7), DailyCap::Limited(3))
        .await
        .unwrap();
    assert_eq!(count(&h, "accounts").await, "3");

    // The last second of the day is still capped; the first of the next day is fresh.
    clock.set(DAY + 86_399);
    assert!(store.open_cap_reached(cap).await.unwrap());
    assert!(matches!(
        store.open_account(name("late"), &fingerprint(8), cap).await,
        Err(OpenError::CapReached)
    ));
    clock.set(DAY + 86_400);
    assert!(!store.open_cap_reached(cap).await.unwrap());
    store
        .open_account(name("fresh"), &fingerprint(9), cap)
        .await
        .unwrap();
    assert_eq!(opened_today(&h, day + 1).await, "1");
    // Filling the new day writes its own single row.
    store
        .open_account(name("fresh2"), &fingerprint(10), cap)
        .await
        .unwrap();
    for n in 11..=13_u32 {
        assert!(matches!(
            store
                .open_account(name(&format!("fresh{n}")), &fingerprint(n), cap)
                .await,
            Err(OpenError::CapReached)
        ));
    }
    assert_eq!(count_of(&all(store).await, Action::RegistrationCapped), 2);
}

/// QA M2 L2: the `registration.capped` row is written after the `account.created` row of the
/// account that used up the cap, so the log reads in the order things happened.
pub(crate) async fn the_capped_row_follows_the_account_created_row_that_used_the_last_place<
    H: Harness,
>() {
    let mut h = H::fresh().await;
    let _clock = clock_at(&mut h, DAY + 10);
    let store = h.store();
    let cap = DailyCap::Limited(2);
    for n in 1..=2_u32 {
        store
            .open_account(name(&format!("member{n}")), &fingerprint(n), cap)
            .await
            .unwrap();
    }
    let rows = all(store).await;
    let id_of = |action: Action, nth_from_last: usize| {
        rows.iter()
            .filter(|r| r.event.action == action)
            .nth(nth_from_last)
            .map(|r| r.id)
            .unwrap()
    };
    // Newest first: the capped row is the newest, and the account that took the last place
    // (the newest `account.created`) is older than it. The first account is older still.
    let capped = id_of(Action::RegistrationCapped, 0);
    let last_created = id_of(Action::AccountCreated, 0);
    let first_created = id_of(Action::AccountCreated, 1);
    assert!(first_created < last_created, "{rows:?}");
    assert!(last_created < capped, "{rows:?}");
}

pub(crate) async fn an_unlimited_cap_never_refuses<H: Harness>() {
    let mut h = H::fresh().await;
    clock_at(&mut h, DAY);
    let store = h.store();
    for n in 1..=30_u32 {
        store
            .open_account(
                name(&format!("member{n}")),
                &fingerprint(n),
                DailyCap::Unlimited,
            )
            .await
            .unwrap();
    }
    assert!(!store.open_cap_reached(DailyCap::Unlimited).await.unwrap());
    assert_eq!(count_of(&all(store).await, Action::RegistrationCapped), 0);
    assert_eq!(count(&h, "accounts").await, "30");
}

pub(crate) async fn a_refused_open_registration_leaves_no_trace_and_costs_no_place<H: Harness>() {
    let mut h = H::fresh().await;
    clock_at(&mut h, DAY);
    let store = h.store();
    let cap = DailyCap::Limited(5);
    let day = Day::of(UnixSeconds::from_raw(DAY)).as_raw();
    let alice = store
        .create_account(NewAccount {
            username: name("alice"),
            role: Slug::MEMBER,
            state: AccountState::Active,
        })
        .await
        .unwrap();
    store
        .attach_fingerprint(alice.id, &fingerprint(1))
        .await
        .unwrap();
    let before = all(store).await.len();

    // A taken handle, a lookalike of one, and a key that already has an account.
    for (handle, key, want) in [
        ("alice", 50_u32, OpenRefusal::Handle),
        ("a1ice", 51, OpenRefusal::Handle),
        ("someone", 1, OpenRefusal::Key),
    ] {
        let refused = store
            .open_account(name(handle), &fingerprint(key), cap)
            .await;
        match refused {
            Err(OpenError::Rejected(got)) => assert_eq!(got, want, "{handle}"),
            other => panic!("{handle}: {other:?}"),
        }
    }
    // The count went back with the rest: no places were spent, no row, no audit row.
    assert_eq!(opened_today(&h, day).await, "none".to_owned());
    assert_eq!(count(&h, "accounts").await, "1");
    assert_eq!(all(store).await.len(), before);
}

pub(crate) async fn concurrent_opens_never_pass_the_cap<H: Harness>() {
    let mut h = H::fresh().await;
    clock_at(&mut h, DAY);
    let store = h.store();
    let mut tasks = Vec::new();
    for n in 0..14_u32 {
        let store = store.clone();
        tasks.push(tokio::spawn(async move {
            store
                .open_account(
                    name(&format!("racer{n}")),
                    &fingerprint(200 + n),
                    DailyCap::Limited(5),
                )
                .await
        }));
    }
    let (mut made, mut capped) = (0, 0);
    for task in tasks {
        match task.await.unwrap() {
            Ok(_) => made += 1,
            Err(OpenError::CapReached) => capped += 1,
            Err(other) => panic!("{other:?}"),
        }
    }
    assert_eq!((made, capped), (5, 9));
    assert_eq!(count(&h, "accounts").await, "5");
    assert_eq!(count(&h, "account_keys").await, "5");
    assert_eq!(
        opened_today(&h, Day::of(UnixSeconds::from_raw(DAY)).as_raw()).await,
        "5"
    );
    assert_eq!(count_of(&all(store).await, Action::RegistrationCapped), 1);
}

pub(crate) async fn old_counter_rows_are_pruned_by_the_next_creation<H: Harness>() {
    let mut h = H::fresh().await;
    clock_at(&mut h, DAY);
    let day = Day::of(UnixSeconds::from_raw(DAY)).as_raw();
    h.raw(&format!(
        "INSERT INTO registration_day (day, opened) VALUES ({}, 3), ({}, 4)",
        day - 30,
        day - 2
    ))
    .await
    .unwrap();
    h.store()
        .open_account(name("newcomer"), &fingerprint(1), DailyCap::Limited(5))
        .await
        .unwrap();
    let days = h
        .texts("SELECT CAST(day AS TEXT) FROM registration_day ORDER BY day")
        .await;
    assert_eq!(days, [(day - 2).to_string(), day.to_string()]);
}

pub(crate) async fn open_registration_never_lands_without_its_audit_row<H: Harness>() {
    let mut h = H::fresh().await;
    clock_at(&mut h, DAY);
    h.break_audit_inserts().await;
    let result = h
        .store()
        .open_account(name("newcomer"), &fingerprint(1), DailyCap::Limited(5))
        .await;
    assert!(matches!(result, Err(OpenError::Store(_))), "{result:?}");
    assert_eq!(count(&h, "accounts").await, "0");
    assert_eq!(count(&h, "account_keys").await, "0");
    assert_eq!(count(&h, "registration_day").await, "0");
}

pub(crate) async fn the_mode_and_the_cap_setting_are_read_on_every_call<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    // Defaults: `invite`, no stored cap.
    assert_eq!(
        store.registration_mode().await.unwrap(),
        RegistrationMode::Invite
    );
    assert_eq!(store.open_cap_setting().await.unwrap(), None);
    // A change is seen by the very next call, without a restart (D38).
    for mode in [
        RegistrationMode::Open,
        RegistrationMode::Apply,
        RegistrationMode::Invite,
    ] {
        set_registration_mode(store, mode).await.unwrap();
        assert_eq!(store.registration_mode().await.unwrap(), mode);
    }
    store
        .set_setting(SettingKey::OpenDailyCap, "40")
        .await
        .unwrap();
    assert_eq!(
        store.open_cap_setting().await.unwrap().as_deref(),
        Some("40")
    );
    assert_eq!(SettingKey::OpenDailyCap.as_str(), "open_daily_cap");
    // An unrecognised mode is an error, never `open`.
    store
        .set_setting(SettingKey::RegistrationMode, "wide-open")
        .await
        .unwrap();
    assert!(store.registration_mode().await.is_err());
}

fn application(key: u32, handle: &str, kind: RequestKind) -> NewRequest {
    NewRequest {
        handle: name(handle),
        message: Message::new("I would like to join"),
        fingerprint: fingerprint(key),
        kind,
    }
}

fn terms() -> SubmitTerms {
    SubmitTerms {
        max_pending: 200,
        ttl_secs: PENDING_TTL_SECS,
    }
}

pub(crate) async fn an_application_is_a_queue_row_of_its_kind_with_one_audit_row_and_no_content<
    H: Harness,
>() {
    let mut h = H::fresh().await;
    clock_at(&mut h, DAY + 5);
    let store = h.store();
    let sysop = store
        .create_account(NewAccount {
            username: name("the-sysop"),
            role: Slug::MEMBER,
            state: AccountState::Active,
        })
        .await
        .unwrap();
    let Submitted::Stored(app) = store
        .submit_request(
            &application(1, "applicant", RequestKind::Application),
            terms(),
        )
        .await
        .unwrap()
    else {
        panic!("stored");
    };
    let Submitted::Stored(ask) = store
        .submit_request(&application(2, "asker", RequestKind::Invite), terms())
        .await
        .unwrap()
    else {
        panic!("stored");
    };
    // The queue lists both, each with its kind.
    let queue = store.pending_requests(100).await.unwrap();
    let kinds: Vec<_> = queue.requests.iter().map(|r| (r.id, r.kind)).collect();
    assert_eq!(
        kinds,
        [(app, RequestKind::Application), (ask, RequestKind::Invite)]
    );
    // Only the application is audited: by the board, naming the request id, nothing typed.
    let rows = all(store).await;
    let submitted: Vec<_> = rows
        .iter()
        .filter(|r| r.event.action == Action::ApplicationSubmitted)
        .collect();
    assert_eq!(submitted.len(), 1);
    assert_eq!(submitted[0].event.actor, Actor::System);
    assert_eq!(submitted[0].event.target, Target::Request(app));
    assert_eq!(submitted[0].event.detail.as_str(), "");
    assert_eq!(submitted[0].at.as_raw(), DAY + 5);
    let dump = format!("{rows:?}");
    assert!(!dump.contains("applicant") && !dump.contains("join"));
    assert!(!dump.contains(fingerprint(1).as_str()));
    // One pending request per key holds across kinds, and a refusal writes no audit row.
    let again = store
        .submit_request(&application(1, "applicant2", RequestKind::Invite), terms())
        .await
        .unwrap();
    assert!(matches!(again, Submitted::Refused(_)));
    assert_eq!(count_of(&all(store).await, Action::ApplicationSubmitted), 1);
    // The sysop's approval is the one M2-E built: an active account invited by the approver.
    let account = store.approve_request(app, sysop.id).await.unwrap();
    assert_eq!(account.state, AccountState::Active);
    assert_eq!(
        top_role_of(store, &fingerprint(1)).await,
        Some(Slug::MEMBER)
    );
    assert_eq!(
        h.texts(&format!(
            "SELECT CAST(invited_by AS TEXT) FROM accounts WHERE id = {}",
            account.id
        ))
        .await,
        [sysop.id.to_string()]
    );
}

pub(crate) async fn an_application_never_lands_without_its_audit_row<H: Harness>() {
    let h = H::fresh().await;
    let store = h.store();
    h.break_audit_inserts().await;
    let result = store
        .submit_request(
            &application(1, "applicant", RequestKind::Application),
            terms(),
        )
        .await;
    assert!(result.is_err(), "{result:?}");
    assert_eq!(count(&h, "invite_request").await, "0");
    // An invite request is not audited, so the broken log does not stop it (D71).
    assert!(matches!(
        store
            .submit_request(&application(2, "asker", RequestKind::Invite), terms())
            .await
            .unwrap(),
        Submitted::Stored(_)
    ));
}

pub(crate) async fn registration_data_survives_a_reopen<H: Harness>() {
    let mut h = H::fresh().await;
    clock_at(&mut h, DAY);
    let cap = DailyCap::Limited(1);
    h.store()
        .open_account(name("newcomer"), &fingerprint(1), cap)
        .await
        .unwrap();
    h.reopen().await;
    clock_at(&mut h, DAY + 60);
    let store = h.store();
    assert!(store.open_cap_reached(cap).await.unwrap());
    assert!(matches!(
        store
            .open_account(name("second"), &fingerprint(2), cap)
            .await,
        Err(OpenError::CapReached)
    ));
}
